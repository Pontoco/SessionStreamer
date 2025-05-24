import { useParams } from '@solidjs/router';
import { createSignal, createMemo, onCleanup, createEffect, Show, type JSX, type Accessor, type Setter } from 'solid-js';
import { useSession, type SessionData, type SessionMetadata } from '../hooks/useSession';
import { Loading } from '../components/common/Loading';
import { NotFound } from '../components/common/NotFound';
import { SolidLogViewer, type LogEntry } from '../components/common/SolidLogViewer';

interface FilterControlsProps {
  filterText: Accessor<string>;
  setFilterText: Setter<string>;
  showTimestamps: Accessor<boolean>;
  setShowTimestamps: Setter<boolean>;
  scrollWithVideo: Accessor<boolean>;
  setScrollWithVideo: Setter<boolean>;
}

function FilterControls(props: FilterControlsProps): JSX.Element {
  return (
    <div class="flex flex-wrap gap-2.5 mb-4">
      <input
        type="text"
        placeholder="Filter logs..."
        value={props.filterText()}
        onInput={(e) => props.setFilterText(e.currentTarget.value)}
        class="block w-full sm:w-[300px] rounded-md border-neutral-300 shadow-sm focus:border-brand-500 focus:ring-brand-500 sm:text-sm p-2"
      />
      <label class="flex items-center gap-2 text-sm text-neutral-700 cursor-pointer select-none">
        <input
          type="checkbox"
          checked={props.showTimestamps()}
          onChange={(e) => props.setShowTimestamps(e.currentTarget.checked)}
          class="rounded border-neutral-300 text-brand-600 shadow-sm focus:ring-brand-500"
        />
        Show Timestamps
      </label>
      <label class="flex items-center gap-2 text-sm text-neutral-700 cursor-pointer select-none">
        <input
          type="checkbox"
          checked={props.scrollWithVideo()}
          onChange={(e) => props.setScrollWithVideo(e.currentTarget.checked)}
          class="rounded border-neutral-300 text-brand-600 shadow-sm focus:ring-brand-500"
        />
        Scroll With Video
      </label>
    </div>
  );
}

interface LogPaneProps {
  rawLogContent: Accessor<string | null | undefined>;
  sessionMetadata: Accessor<SessionMetadata | null | undefined>;
  videoRef: Accessor<HTMLVideoElement | undefined>;
  filterText: Accessor<string>;
  setFilterText: Setter<string>;
  showTimestamps: Accessor<boolean>;
  setShowTimestamps: Setter<boolean>;
  scrollWithVideo: Accessor<boolean>;
  setScrollWithVideo: Setter<boolean>;
  isLoading: Accessor<boolean>;
}

function LogPane(props: LogPaneProps): JSX.Element {
  const [targetLogIndexToScroll, setTargetLogIndexToScroll] = createSignal<number | null>(null);
  const logLineRegex = /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+[+-]\d{2}:\d{2})\s*(.*)$/;

  const processedLogs = createMemo((): LogEntry[] => {
    const content = props.rawLogContent();
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
    const term = props.filterText().toLowerCase();
    if (!term) {
      return allLogs;
    }
    return allLogs.filter(log => log.message.toLowerCase().includes(term));
  });

  createEffect(() => {
    const sMeta = props.sessionMetadata();
    const videoElement = props.videoRef();

    if (!videoElement || !sMeta || !sMeta.timestamp || !props.scrollWithVideo()) {
      return;
    }

    const baseTimestampStr = sMeta.timestamp as string;
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

  return (
    <div class="flex flex-col h-full">
      <h2 class="text-heading-3 mb-4">Unity Log</h2>
      <FilterControls
        filterText={props.filterText}
        setFilterText={props.setFilterText}
        showTimestamps={props.showTimestamps}
        setShowTimestamps={props.setShowTimestamps}
        scrollWithVideo={props.scrollWithVideo}
        setScrollWithVideo={props.setScrollWithVideo}
      />
      <div class="flex-grow min-h-0">
        <SolidLogViewer
          class="h-full"
          logs={processedLogs}
          isLoading={props.isLoading}
          showTimestamps={props.showTimestamps}
          targetLogIndex={targetLogIndexToScroll}
          placeholder={props.rawLogContent() === null && !props.isLoading() ? "Log file could not be loaded or is empty." : "No logs to display."}
        />
      </div>
       <Show when={!props.isLoading() && props.rawLogContent() === null && props.sessionMetadata()?.log_url}>
         <p class="mt-2 text-sm text-neutral-500">Log file specified but could not be loaded.</p>
       </Show>
    </div>
  );
}

interface VideoPaneProps {
  src: Accessor<string | undefined>;
  ref: (el: HTMLVideoElement) => void;
}

function VideoPane(props: VideoPaneProps): JSX.Element {
  return (
    <section class="space-y-3">
      <h2 class="text-heading-3">Video</h2>
      <Show when={props.src()} fallback={<p>Loading video...</p>}>
        <video ref={props.ref} controls src={props.src()} class="rounded w-full bg-black" style="max-height: 60vh;">
          Your browser does not support the video tag.
        </video>
      </Show>
    </section>
  );
}

interface MetadataPaneProps {
  metadata: Accessor<SessionMetadata | null | undefined>;
}

function MetadataPane(props: MetadataPaneProps): JSX.Element {
  return (
    <section class="space-y-3">
      <h2 class="text-heading-3">Metadata</h2>
      <Show when={props.metadata()} fallback={<p>Loading metadata...</p>}>
        {(meta) => (
          <pre class="p-4 bg-neutral-100 dark:bg-neutral-800 rounded-md overflow-x-auto text-sm border border-neutral-200 dark:border-neutral-700">
            {JSON.stringify(meta(), null, 2)}
          </pre>
        )}
      </Show>
    </section>
  );
}


export default function SessionDetailPage(): JSX.Element {
  const params = useParams();
  const { sessionData, rawLogContent } = useSession(() => params.session_id);

  const [showTimestamps, setShowTimestamps] = createSignal(false);
  const [scrollWithVideo, setScrollWithVideo] = createSignal(true);
  const [filterText, setFilterText] = createSignal('');
  let videoRefInternal: HTMLVideoElement | undefined;

  const setVideoRef = (el: HTMLVideoElement) => {
    videoRefInternal = el;
  };
  const getVideoRef = () => videoRefInternal;


  return (
    <Loading
      when={sessionData.loading}
      fallback={<p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">Loading session details...</p>}
    >
      <NotFound
        when={!sessionData()}
        fallback={<p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">Session not found or failed to load.</p>}
      >
        <main class="h-screen flex flex-col p-6 bg-neutral-50 dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200">
          <h1 class="text-heading-2 mb-4">Session: {sessionData()!.metadata.session_id}</h1>
          <div class="flex-grow grid lg:grid-cols-[1fr_minmax(0,_2fr)] gap-6 overflow-hidden">
            <div class="flex flex-col gap-6 overflow-y-auto p-1">
              <VideoPane src={() => sessionData()!.video_url} ref={setVideoRef} />
              <MetadataPane metadata={() => sessionData()!.metadata} />
            </div>
            <div class="flex flex-col min-h-0">
               <LogPane
                  rawLogContent={rawLogContent}
                  sessionMetadata={() => sessionData()!.metadata}
                  videoRef={getVideoRef}
                  filterText={filterText}
                  setFilterText={setFilterText}
                  showTimestamps={showTimestamps}
                  setShowTimestamps={setShowTimestamps}
                  scrollWithVideo={scrollWithVideo}
                  setScrollWithVideo={setScrollWithVideo}
                  isLoading={() => rawLogContent.loading}
                />
            </div>
          </div>
        </main>
      </NotFound>
    </Loading>
  );
}