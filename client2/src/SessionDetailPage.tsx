import { useParams } from '@solidjs/router';
import { createResource, Show, createSignal, createMemo, onCleanup, createEffect } from 'solid-js';
import type { JSX } from 'solid-js';
import { SolidLogViewer, type LogEntry } from './SolidLogViewer';

interface SessionMetadata {
  session_id: string;
  [key: string]: any;
}

interface SessionData {
  metadata: SessionMetadata;
  video_url: string;
  log_url: string;
}

async function fetchSessionData(sessionId: string): Promise<SessionData | null> {
  console.log(`Fetching session data for ${sessionId}`);
  try {
    const response = await fetch(`/rest/session/${sessionId}`);
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch session data for ${sessionId}:`, error);
    return null;
  }
}

async function fetchLogContent(logUrl: string): Promise<string> {
  try {
    const response = await fetch(logUrl);
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status} for ${logUrl}`);
    }
    return await response.text();
  } catch (error) {
    console.error(`Failed to fetch log content from ${logUrl}:`, error);
    return `Error loading log: ${error instanceof Error ? error.message : String(error)}`;
  }
}

export default function SessionDetailPage(): JSX.Element {
  const params = useParams();
  const [sessionData] = createResource(() => params.session_id, fetchSessionData);
  const [rawLogContent] = createResource(() => sessionData()?.log_url, fetchLogContent);
  const [showTimestamps, setShowTimestamps] = createSignal(false);
  const [filterText, setFilterText] = createSignal('');
  const [scrollWithVideo, setScrollWithVideo] = createSignal(true);
  const [targetLogIndexToScroll, setTargetLogIndexToScroll] = createSignal<number | null>(null);

  let videoRef: HTMLVideoElement | undefined;

  const logLineRegex = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+[+-]\d{2}:\d{2})\s*(.*)$/;

  const processedLogs = createMemo((): LogEntry[] => {
    const content = rawLogContent();
    if (!content) return [];

    const lines = content.split('\n');
    const allLogs: LogEntry[] = [];

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      const match = line.match(logLineRegex);
      if (match) {
        allLogs.push({
          id: i,
          timestamp: new Date(match[1]),
          message: match[2] || '', // Ensure message is not undefined
        });
      } else {
        allLogs.push({
          id: i,
          message: line,
        });
      }
    }

    const term = filterText().toLowerCase();
    if (!term) {
      return allLogs;
    }
    return allLogs.filter(log => log.message.toLowerCase().includes(term));
  });

  createEffect(() => {
    const sData = sessionData();
    const videoElement = videoRef;
    console.log("Video Element:", videoElement);

    if (!videoElement || !sData || !sData.metadata.timestamp || !scrollWithVideo()) {
      // If not scrolling with video, or necessary data/refs are missing, do nothing.
      // Or remove listener if it was previously attached and scrollWithVideo is now false.
      if (videoElement && videoElement.onplay) { // Check if a handler was attached
        // How to properly remove specific handler if we don't store it?
        // For now, this effect reruns and won't add if scrollWithVideo is false.
      }
      return;
    }

    const baseTimestampStr = sData.metadata.timestamp as string; // Already confirmed it's a string
    let baseTimeMs: number;
    try {
      baseTimeMs = Date.parse(baseTimestampStr);
      if (isNaN(baseTimeMs)) {
        console.error("Invalid base timestamp in metadata:", baseTimestampStr);
        return;
      }
    } catch (e) {
      console.error("Error parsing base timestamp:", baseTimestampStr, e);
      return;
    }

    const handleTimeUpdate = () => {
      if (!videoElement) return;
      const currentVideoTimeMs = videoElement.currentTime * 1000;
      const currentAbsoluteTime = new Date(baseTimeMs + currentVideoTimeMs);
      
      const logs = processedLogs();
      if (!logs.length) {
        setTargetLogIndexToScroll(null);
        return;
      }

      let foundIndex: number | null = null;
      // Iterate backwards to find the last log entry at or before the current video time
      for (let i = logs.length - 1; i >= 0; i--) {
        const log = logs[i];
        if (log.timestamp && log.timestamp instanceof Date && !isNaN(log.timestamp.getTime())) {
          if (log.timestamp.getTime() <= currentAbsoluteTime.getTime()) {
            foundIndex = i;
            break;
          }
        }
      }
      
      // If no log is at or before, but video has started, maybe scroll to first if it's later?
      // Or if all logs are before current time, scroll to last.
      if (foundIndex === null && logs.length > 0 && logs[0].timestamp && logs[0].timestamp.getTime() > currentAbsoluteTime.getTime()) {
        // Video time is before the first log with a timestamp
        // setTargetLogIndexToScroll(0); // Option: scroll to first log
        setTargetLogIndexToScroll(null); // Option: don't scroll yet
      } else {
        setTargetLogIndexToScroll(foundIndex);
      }
    };

    videoElement.addEventListener('timeupdate', handleTimeUpdate);
    onCleanup(() => {
      videoElement.removeEventListener('timeupdate', handleTimeUpdate);
    });
  });

  createEffect(() => {
    console.log("url::"+ sessionData()?.video_url);
  });

  return (
    <div class="p-5">
      <Show when={sessionData.loading}>
        <p>Loading session details...</p>
      </Show>
      <Show when={!sessionData.loading && sessionData()}>
        {(data) => (
          <div>
            <h1 class="text-2xl font-bold mb-4">Session: {data().metadata.session_id}</h1>
            <div class="flex flex-wrap gap-5">
              <div class="flex-1 basis-[600px] min-w-[300px]">
                <h2 class="text-xl font-semibold mb-2">Video</h2>
                <video ref={videoRef} controls width="100%" src={data().video_url}>
                  Your browser does not support the video tag.
                </video>
              </div>
              <div class="flex-1 basis-[400px] min-w-[300px]">
                <h2 class="text-xl font-semibold mb-2">Unity Log</h2>
                <div class="flex flex-col gap-2.5 mb-2.5">
                  <div class="flex items-center flex-wrap gap-2.5"> {/* Added flex-wrap */}
                    <input
                      type="text"
                      placeholder="Filter logs..."
                      value={filterText()}
                      onInput={(e) => setFilterText(e.currentTarget.value)}
                      class="p-2 rounded border border-gray-300 w-[300px]"
                    />
                    <label class="flex items-center gap-[5px] cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={showTimestamps()}
                        onChange={(e) => setShowTimestamps(e.currentTarget.checked)}
                      />
                      Show Timestamps
                    </label>
                    <label class="flex items-center gap-[5px] cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={scrollWithVideo()}
                        onChange={(e) => setScrollWithVideo(e.currentTarget.checked)}
                      />
                      Scroll With Video
                    </label>
                  </div>
                </div>
                <SolidLogViewer
                  logs={processedLogs}
                  isLoading={() => rawLogContent.loading}
                  containerHeight="500px"
                  showTimestamps={showTimestamps}
                  targetLogIndex={targetLogIndexToScroll}
                />
                 <Show when={!rawLogContent.loading && rawLogContent() === undefined && sessionData()?.log_url}>
                    <p>Log file specified but could not be loaded.</p>
                 </Show>
              </div>
            </div>
            <h2 class="text-xl font-semibold mt-7 mb-2">Metadata</h2>
            <pre class="border border-gray-300 p-2.5 bg-gray-50 rounded overflow-x-auto">
              {JSON.stringify(data().metadata, null, 2)}
            </pre>
          </div>
        )}
      </Show>
      <Show when={!sessionData.loading && !sessionData()}>
        <p>Session not found or failed to load.</p>
      </Show>
    </div>
  );
}