import { createSignal, createEffect, onCleanup, For, Show, createMemo, Accessor, JSX, ComponentProps, splitProps } from 'solid-js';
import { createVirtualizer, VirtualItem } from '@tanstack/solid-virtual';
import { cn } from './solid_ui/utils';

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
  showTimestamps?: Accessor<boolean>; // Prop to control timestamp visibility
  defaultItemHeight?: number; // For virtualizer
  containerHeight?: string; // e.g., "600px"
  placeholder?: string; // Placeholder when logs are empty
  targetLogIndex?: Accessor<number | undefined | null>; // For programmatic scrolling
}

export function SolidLogViewer(props: LogViewerProps) {
  const [local, others] = splitProps(props, ["class"])
  const [isSearchActive, setIsSearchActive] = createSignal(false);
  const [searchTerm, setSearchTerm] = createSignal('');
  let parentRef: HTMLDivElement | undefined = undefined;
  let searchInputRef: HTMLInputElement | undefined;

  const defaultShowTimestamps = () => props.showTimestamps !== undefined ? props.showTimestamps() : true;
  const itemHeight = () => props.defaultItemHeight || 22; // Estimated height of a single log line
  const placeholderMessage = () => props.placeholder || 'No logs to display.';

  // Filter logs based on search term
  const filteredLogs = createMemo(() => {
    const term = searchTerm().toLowerCase();
    if (!term) {
      return props.logs();
    }
    return props.logs().filter((log: LogEntry) => log.message.toLowerCase().includes(term));
  });

  const initialVirtualizerOptions = {
    getScrollElement: () => parentRef ?? null,
    estimateSize: (_index: number) => itemHeight(),
    overscan: 100,
    // `count` will be part of the reactive updates
    // `scrollToFn`, `observeElementRect`, `observeElementOffset` will use defaults
    // or need to be explicitly set if defaults are not sufficient or if they are required by setOptions.
    // For now, let's assume they are handled by the library if not specified in the initial object for createVirtualizer
    // and that setOptions needs them if they were part of the "full" options set.
    // The error implies setOptions needs a *complete* set if it's replacing.
  };

  console.log('Creating virtualizer with options:', initialVirtualizerOptions);
  const rowVirtualizer = createVirtualizer<HTMLDivElement, Element>({
    ...initialVirtualizerOptions,
    count: filteredLogs().length, // Initial count
  });

  // Effect to update virtualizer options when log count or other relevant reactive sources change
  createEffect(() => {
    console.log('Creating virtualizer with count:', filteredLogs().length);
    rowVirtualizer.setOptions({
      ...rowVirtualizer.options, // Spread the current options from the instance
      count: filteredLogs().length, // Override the count
    });
    // After updating options, especially the count, we might need to tell the virtualizer to re-measure.
    if (parentRef && filteredLogs().length > 0) {
      rowVirtualizer.measure();
    }
  });

  // Effect to scroll to a target index when props.targetLogIndex changes
  createEffect(() => {
    const targetIndex = props.targetLogIndex ? props.targetLogIndex() : null;
    if (typeof targetIndex === 'number' && targetIndex >= 0 && targetIndex < filteredLogs().length) {
      // Check if rowVirtualizer is initialized and has items
      if (rowVirtualizer && rowVirtualizer.getVirtualItems().length > 0) {
        // A short delay can sometimes help ensure the DOM is ready for scrolling, especially after data changes.
        setTimeout(() => {
          rowVirtualizer.scrollToIndex(targetIndex, { align: 'start', behavior: 'smooth' });
        }, 0);
      }
    }
  });

  // Handle Ctrl+F / Cmd+F for search
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

  // Focus search input when it becomes active
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
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', fractionalSecondDigits: 3 });
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

      <div class="flex-grow overflow-y-scroll" style="contain: size; contain-intrinsic-size: 50px">
        <For each={filteredLogs()}>
          {(log: LogEntry, index) => (
            <div>
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
        test
      </div>


      {/* <Show when={props.isLoading()} fallback={
        <div
          ref={parentRef}
          class="flex-grow min-h-0 overflow-auto" // Added relative for consistency, though inner div has it for items
        >
          <Show when={filteredLogs().length > 0} fallback={
             <div class="justify-center items-center h-full text-gray-600 font-sans">
               {searchTerm() ? 'No matching logs.' : placeholderMessage()}
             </div>
          }>
            <div // The main div that is the size of the log lines.
              style={{ // These styles are essential for the virtualizer's calculations
                height: `${rowVirtualizer.getTotalSize()}px`,
                width: '100%',
                position: 'relative',
              }}
            >
              <For each={rowVirtualizer.getVirtualItems()}>
                {(virtualRow: VirtualItem) => {
                  const log = filteredLogs()[virtualRow.index];
                  return (
                    <div
                      class="px-2 flex items-start hover:bg-gray-50 box-border"
                      style={{
                        position: 'absolute',
                        top: '0',
                        left: '0',
                        width: '100%',
                        // height is determined by content & virtualizer measurement
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                      data-index={virtualRow.index}
                    >
                      <Show when={defaultShowTimestamps() && log?.timestamp}>
                        <span class="mr-2.5 text-gray-500 whitespace-nowrap">
                          {log ? formatTimestamp(log!.timestamp!) : ''}
                        </span>
                      </Show>
                      <span class="flex-grow whitespace-nowrap"> 
                        {log ? highlightMatch(log!.message, searchTerm()) : ''}
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </Show>
        </div>
      }>
        <div class="flex justify-center items-center text-gray-600 font-sans" >
          Loading logs...
        </div>
      </Show> */}
    </div>
  );
}