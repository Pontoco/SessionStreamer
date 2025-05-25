import { useParams, A, useSearchParams } from '@solidjs/router';
import { createSignal, createMemo, onCleanup, createEffect, Show, type JSX, type Accessor, type Setter, For } from 'solid-js';
import { useSession, type SessionData, type SessionMetadata } from '../hooks/useSession';
import { useAllSessions } from '../hooks/useSessionList';
import { Icon } from 'solid-heroicons';
import { queueList } from 'solid-heroicons/solid';
import { Loading } from '../components/common/Loading';
import { NotFound } from '../components/common/NotFound';
import { SolidLogViewer, type LogEntry } from '../components/common/SolidLogViewer';
import {
  SidebarProvider,
  Sidebar,
  SidebarTrigger,
  SidebarHeader,
  SidebarContent,
  SidebarFooter,
  SidebarMenu,
  SidebarMenuItem,
  SidebarMenuButton,
  SidebarInset
} from '../solid_ui/sidebar';
import {
  Breadcrumb,
  BreadcrumbList,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbSeparator
} from '../solid_ui/breadcrumb';

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

    if (!videoElement || !sMeta || !sMeta.timestamp_utc || !props.scrollWithVideo()) {
      console.log("Skipping time update setup: videoElement, session metadata, or scrollWithVideo condition not met.");
      console.log("videoElement:", videoElement, "sessionMetadata:", sMeta, "scrollWithVideo:", props.scrollWithVideo());
      return;
    }

    const baseTimestampStr = sMeta.timestamp_utc as string;
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
      console.log("Video time updated:", videoElement.currentTime);
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


      console.log("Found log index to scroll to:", foundIndex, "for current time:", currentAbsoluteTime.toISOString());
      if (foundIndex === null && logs.length > 0 && logs[0].timestamp && logs[0].timestamp.getTime() > currentAbsoluteTime.getTime()) {
        setTargetLogIndexToScroll(null);
      } else {
        setTargetLogIndexToScroll(foundIndex);
      }
    };

    console.log("Setting up timeupdate listener for video element:", videoElement);
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
      <div class="flex-grow">
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

function VideoPane(props: {
  src: Accessor<string | undefined>;
  ref: (el: HTMLVideoElement) => void;
}): JSX.Element {
  return (
    <section class="space-y-3">
      <h2 class="text-heading-3">Video</h2>
      <Show when={props.src()} fallback={<p>Loading video...</p>}>
        <video ref={props.ref} controls src={props.src()} class="rounded  bg-black" style="max-height: 60vh;">
          Your browser does not support the video tag.
        </video>
      </Show>
    </section>
  );
}

function MetadataPane(props: {
  metadata: Accessor<SessionMetadata | null | undefined>;
}): JSX.Element {
  return (
    <section class="space-y-3">
      <h2 class="text-heading-3">Metadata</h2>
      <Show when={props.metadata()} fallback={<p>Loading metadata...</p>}>
        {(meta) => {
          const currentMeta = meta();
          if (!currentMeta) return <p>No metadata available.</p>;

          return (
            <dl class="text-xs text-neutral-700 dark:text-neutral-300 space-y-1 p-2 bg-neutral-100 dark:bg-neutral-800 rounded-md border border-neutral-200 dark:border-neutral-700">
              {Object.entries(currentMeta).map(([key, value]) => {
                let displayValue: string;
                if (value === null || value === undefined) {
                  displayValue = "N/A";
                } else if (typeof value === 'object') {
                  displayValue = JSON.stringify(value);
                } else if (typeof value === 'function') {
                  displayValue = "[Function]";
                } else {
                  displayValue = String(value);
                }
                return (
                  <div class="flex">
                    <dt class="font-medium w-1/3 truncate pr-2">{key.replace(/_/g, ' ').replace(/\b\w/g, l => l.toUpperCase())}:</dt>
                    <dd class="w-2/3 truncate">{displayValue}</dd>
                  </div>
                );
              })}
            </dl>
          );
        }}
      </Show>
    </section>
  );
}

interface SessionDetailSidebarProps {
  currentSessionMetadata: Accessor<SessionMetadata | null | undefined>;
  currentProjectId: Accessor<string | undefined>;
}

function SessionDetailSidebar(props: SessionDetailSidebarProps): JSX.Element {
  const allSessions = useAllSessions(props.currentProjectId);

  const currentUsername = createMemo(() => props.currentSessionMetadata()?.username);

  const relatedSessions = createMemo(() => {
    const username = currentUsername();
    const sessions = allSessions();

    if (!username || !sessions) return [];

    return sessions
      .filter((s) => s.username === username)
      .sort((a, b) => new Date(b.timestamp_utc).getTime() - new Date(a.timestampUtc).getTime());
  });

  return (
    <Sidebar>
      <SidebarHeader>
        <Show
          when={currentUsername()}
          fallback={<h2 class="text-lg font-semibold px-2 py-1">Session Info</h2>}
        >
          <h2 class="text-lg font-semibold px-2 py-1">Sessions for {currentUsername()}</h2>
        </Show>
      </SidebarHeader>
      <SidebarContent>
        <Show
          when={currentUsername()}
          fallback={
            <p class="p-3 text-sm text-neutral-500">
              Username not found in this session's metadata. Cannot display related sessions.
            </p>
          }
        >
          <Show when={allSessions.loading}>
            <p class="p-3 text-sm text-neutral-500">Loading other sessions...</p>
          </Show>
          <Show when={!allSessions.loading && allSessions.error}>
            <p class="p-3 text-sm text-red-500">Error loading session list.</p>
          </Show>
          <Show when={!allSessions.loading && !allSessions.error && relatedSessions().length === 0}>
            <p class="p-3 text-sm text-neutral-500">No other sessions found for {currentUsername()}.</p>
          </Show>
          <Show when={!allSessions.loading && !allSessions.error && relatedSessions().length > 0}>
            <SidebarMenu class="gap-4" >
              <For each={relatedSessions()}>
                {(session: SessionMetadata, i) => (
                  <SidebarMenuItem>
                    <A href={`/session/${session.session_id}`} class="w-full block">
                      <SidebarMenuButton class="w-full text-left">
                        <div class="flex flex-col">
                          <span class="">
                            <b>{(i() + 1)}</b>: {new Date(session.timestamp_utc).toLocaleString()}
                          </span>
                          <span class="text-xs text-neutral-500 dark:text-neutral-400">ID: {session.session_id.substring(0, 8)}...</span>
                        </div>
                      </SidebarMenuButton>
                    </A>
                  </SidebarMenuItem>
                )}
              </For>
            </SidebarMenu>
          </Show>
        </Show>
      </SidebarContent>
      <SidebarFooter>
        <p class="text-xs text-neutral-500 p-2">SessionStreamer v1.0</p>
      </SidebarFooter>
    </Sidebar>
  );
}

export default function SessionDetailPage(): JSX.Element {
  const params = useParams();
  const { sessionData, rawLogContent } = useSession(() => params.session_id, () => params.project_id as string);

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
        <SidebarProvider defaultOpen={false}>
          <div class="flex h-screen">
            <SessionDetailSidebar
              currentSessionMetadata={() => sessionData()?.metadata}
              currentProjectId={() => params.project_id as string}
            />
            <SidebarInset class="flex-grow flex flex-col overflow-auto">
              <div class="flex items-center justify-start gap-4 p-4 border-b border-neutral-200 dark:border-neutral-700">
                <SidebarTrigger />
                <Breadcrumb>
                  <BreadcrumbList>
                    <BreadcrumbItem>
                      <BreadcrumbLink as={A} href={`/sessions/${params.project_id}`} class="flex items-center gap-1.5">
                        Sessions
                      </BreadcrumbLink>
                    </BreadcrumbItem>
                    <BreadcrumbSeparator />
                    <Show when={sessionData()?.metadata?.username}>
                      <BreadcrumbItem>
                        <span>{sessionData()!.metadata.username}</span>
                      </BreadcrumbItem>
                      <BreadcrumbSeparator />
                    </Show>
                    <BreadcrumbItem>
                      <span>{params.session_id}</span>
                    </BreadcrumbItem>
                  </BreadcrumbList>
                </Breadcrumb>
              </div>
              <div class="p-6 flex-grow flex flex-col bg-neutral-50 dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200">
                <div class="flex-grow grid lg:grid-cols-2 gap-6 overflow-hidden">
                  <div class="flex flex-grow flex-col gap-6 overflow-y-auto p-1">
                    <VideoPane src={() => sessionData()!.video_url} ref={setVideoRef} />
                    <MetadataPane metadata={() => sessionData()!.metadata} />
                  </div>
                  <div class="flex flex-grow flex-col">
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
              </div>
            </SidebarInset>
          </div>
        </SidebarProvider>
      </NotFound>
    </Loading>
  );
}