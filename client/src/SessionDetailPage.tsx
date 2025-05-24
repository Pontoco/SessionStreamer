import { useParams } from '@solidjs/router';
import { createResource, Show, createSignal, createMemo, onCleanup, createEffect } from 'solid-js';
import type { JSX } from 'solid-js';
import { SolidLogViewer, type LogEntry } from './SolidLogViewer';
import Flex from './Flex';

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
          message: match[2] || '',
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
      if (videoElement && videoElement.onplay) {
      }
      return;
    }

    const baseTimestampStr = sData.metadata.timestamp as string;
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
      for (let i = logs.length - 1; i >= 0; i--) {
        const log = logs[i];
        if (log.timestamp && log.timestamp instanceof Date && !isNaN(log.timestamp.getTime())) {
          if (log.timestamp.getTime() <= currentAbsoluteTime.getTime()) {
            foundIndex = i;
            break;
          }
        }
      }

      if (foundIndex === null && logs.length > 0 && logs[0].timestamp && logs[0].timestamp.getTime() > currentAbsoluteTime.getTime()) {
        setTargetLogIndexToScroll(null);
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
    console.log("url::" + sessionData()?.video_url);
  });

  // return (
  //   <div class="flex flex-col" style="height: 100vh; width: 100vw;">
  //     <div class="flex flex-row flex-grow">
  //       <div class="" style="background-color:blue">left</div>
  //       <div class="" id="log_holder" style="background-color:white; padding: 15px; overflow:scroll;">
  //         <div style="height: 15000px; width: 5000px; background-color:red"></div>
  //       </div>
  //     </div>
  //   </div>
  // );

  return (
    <div id="page" class="flex flex-col" style="height: 100vh; width: 100vw;">
      <Show when={sessionData.loading}>
        <p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">Loading session details...</p>
      </Show>
      <Show when={!sessionData.loading && sessionData()}>
        {(data) => (
          <div id="content" class="flex flex-col flex-grow">
            <h1 class="text-heading-2">Session: {data().metadata.session_id}</h1>

            <div id="two-columns" class="flex flex-grow flex-row">
              <div id="left" class="flex-grow">
                <h2 class="text-heading-3 mb-4">Video</h2>
                <video ref={videoRef} controls width="100%" src={data().video_url} class="rounded">
                  Your browser does not support the video tag.
                </video>

                <div class="mt-6 bg-white shadow-lg rounded-lg p-6 sm:p-8">
                  <h2 class="text-heading-3 mb-4">Metadata</h2>
                  <pre class="p-4 bg-neutral-50 rounded-md overflow-x-auto text-sm">
                    {JSON.stringify(data().metadata, null, 2)}
                  </pre>
                </div>
              </div>
              <div id="right" class="flex flex-col flex-grow">
                <h2 class="text-heading-3 mb-4">Unity Log</h2>
                  <div class="flex flex-wrap gap-2.5">
                    <input
                      type="text"
                      placeholder="Filter logs..."
                      value={filterText()}
                      onInput={(e) => setFilterText(e.currentTarget.value)}
                      class="block w-full sm:w-[300px] rounded-md border-neutral-300 shadow-sm focus:border-brand-500 focus:ring-brand-500 sm:text-sm p-2"
                    />
                    <label class="flex items-center gap-2 text-sm text-neutral-700 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={showTimestamps()}
                        onChange={(e) => setShowTimestamps(e.currentTarget.checked)}
                        class="rounded border-neutral-300 text-brand-600 shadow-sm focus:ring-brand-500"
                      />
                      Show Timestamps
                    </label>
                    <label class="flex items-center gap-2 text-sm text-neutral-700 cursor-pointer select-none">
                      <input
                        type="checkbox"
                        checked={scrollWithVideo()}
                        onChange={(e) => setScrollWithVideo(e.currentTarget.checked)}
                        class="rounded border-neutral-300 text-brand-600 shadow-sm focus:ring-brand-500"
                      />
                      Scroll With Video
                    </label>
                  </div>
                <SolidLogViewer
                  class="flex-grow"
                  logs={processedLogs}
                  isLoading={() => rawLogContent.loading}
                  showTimestamps={showTimestamps}
                  targetLogIndex={targetLogIndexToScroll}
                />
                <Show when={!rawLogContent.loading && rawLogContent() === undefined && sessionData()?.log_url}>
                  <p class="mt-2 text-sm text-neutral-500">Log file specified but could not be loaded.</p> {/* Added some margin and styling */}
                </Show>
              </div>
            </div>
          </div>
        )}
      </Show>
      <Show when={!sessionData.loading && !sessionData()}>
        <p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">Session not found or failed to load.</p>
      </Show>
    </div>
  );
}