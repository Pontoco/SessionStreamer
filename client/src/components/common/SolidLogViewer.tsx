import { createSignal, createEffect, onCleanup, For, Show, createMemo, Accessor, JSX, ComponentProps, splitProps } from 'solid-js';
import { cn } from '../../solid_ui/utils';

// Define the structure for a log entry
export interface LogEntry {
  id: string | number;
  timestamp?: Date;
  message: string;
  level?: 'INFO' | 'WARN' | 'ERROR' | 'DEBUG'; // Optional: for styling
}

interface LogViewerProps extends ComponentProps<'div'> {
  logs: Accessor<LogEntry[]>; // Logs should be passed as an accessor (signal/prop)
  isLoading: Accessor<boolean>;
  loadError?: Accessor<string | null | undefined>; // Optional: To display a load error message
  showTimestamps?: Accessor<boolean>; // Prop to control timestamp visibility
  containerHeight?: string; // e.g., "600px"
  placeholder?: string; // Placeholder when logs are empty
  targetLogIndex?: Accessor<number | undefined | null>; // For programmatic scrolling
}

export function SolidLogViewer(props: LogViewerProps) {
  const [local, others] = splitProps(props, ["class"])
  const [isSearchActive, setIsSearchActive] = createSignal(false);
  const [searchTerm, setSearchTerm] = createSignal('');
  let searchInputRef: HTMLInputElement | undefined;

  const defaultShowTimestamps = () => props.showTimestamps !== undefined ? props.showTimestamps() : true;
  const placeholderMessage = () => props.placeholder || 'No logs to display.';

  const filteredLogs = createMemo(() => {
    const term = searchTerm().toLowerCase();
    if (!term) {
      return props.logs();
    }
    return props.logs().filter((log: LogEntry) => log.message.toLowerCase().includes(term));
  });

  createEffect(() => {
    const targetIndex = props.targetLogIndex ? props.targetLogIndex() : null;
    // Ensure targetIndex is a number and within the bounds of filteredLogs
    if (typeof targetIndex === 'number' && targetIndex >= 0 && targetIndex < filteredLogs().length) {
      // Use a timeout to ensure the DOM has rendered the new items before trying to scroll
      setTimeout(() => {
        const element = document.getElementById(`log-entry-${targetIndex}`);
        if (element) {
          element.scrollIntoView({ behavior: 'smooth', block: 'start' });
        }
      }, 0);
    }
  });

  createEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'f') {
        event.preventDefault();
        setIsSearchActive(true);
        searchInputRef?.focus();
      }
      if (event.key === 'Escape' && isSearchActive()) {
        event.preventDefault();
        setIsSearchActive(false);
        setSearchTerm(''); // Clear search on Esc
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    onCleanup(() => {
      window.removeEventListener('keydown', handleKeyDown);
    });
  });

  createEffect(() => {
    if (isSearchActive()) {
      searchInputRef?.focus();
    }
  });

  const highlightMatch = (text: string, term: string): JSX.Element | string => {
    if (!term.trim()) {
      return text;
    }
    // Escape special characters in search term for regex
    const escapedTerm = term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const parts = text.split(new RegExp(`(${escapedTerm})`, 'gi'));
    return (
      <For each={parts}>
        {(part: string) => (
          part.toLowerCase() === term.toLowerCase() ? <strong>{part}</strong> : part
        )}
      </For>
    );
  };

  const formatTimestamp = (date: Date): string => {
    return date.toISOString();
    // return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', fractionalSecondDigits: 3 });
  };

  return (

    <div class={cn("flex flex-col border border-gray-300 font-mono text-sm", props.class)} {...others}>
      <Show when={isSearchActive()}>
        <div class="flex p-2 border-b border-gray-200">
          <input
            ref={searchInputRef}
            type="text"
            placeholder="Search logs..."
            value={searchTerm()}
            onInput={(e: Event & { currentTarget: HTMLInputElement; target: Element; }) => setSearchTerm(e.currentTarget.value)}
            onKeyDown={(e: KeyboardEvent) => {
              if (e.key === 'Enter') e.preventDefault(); // Prevent form submission if any
              if (e.key === 'Escape') {
                setIsSearchActive(false);
                setSearchTerm('');
              }
            }}
            class="flex-grow p-1.5 border border-gray-300 rounded-sm"
          />
          <button
            onClick={() => { setIsSearchActive(false); setSearchTerm(''); }}
            class="ml-2 py-1.5 px-2.5 border border-gray-300 bg-gray-100 hover:bg-gray-200 cursor-pointer rounded-sm"
          >
            ×
          </button>
        </div>
      </Show>

      <div class="flex-grow overflow-y-scroll p-2" style="contain: size; contain-intrinsic-size: 50px">
        <Show when={props.loadError && props.loadError()}>
          <div class="text-red-500 p-2">
            Error loading logs: {props.loadError!()}
          </div>
        </Show>
        <Show when={!props.loadError || !props.loadError()}>
          <Show when={props.isLoading()}>
            <div class="p-2 text-gray-500">Loading logs...</div>
          </Show>
          <Show when={!props.isLoading() && filteredLogs().length === 0}>
            <div class="p-2 text-gray-500">{placeholderMessage()}</div>
          </Show>
          <Show when={!props.isLoading() && filteredLogs().length > 0}>
            <For each={filteredLogs()}>
              {(log: LogEntry, index) => (
                <div id={"log-entry-" + index()} class="flex">
                  <Show when={defaultShowTimestamps() && log?.timestamp}>
                    <span class="mr-2.5 text-gray-500 whitespace-nowrap">
                      {log ? formatTimestamp(log!.timestamp!) : ''}
                    </span>
                  </Show>
                  <span class="flex-grow whitespace-nowrap">
                    {log ? highlightMatch(log!.message, searchTerm()) : ''}
                  </span>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>


    </div>
  );
}