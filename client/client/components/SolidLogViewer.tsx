import { createSignal, createEffect, onCleanup, For, Show, createMemo, Accessor } from 'solid-js';
import { createVirtualizer } from '@tanstack/solid-virtual';
import './SolidLogViewer.css';

// Define the structure for a log entry
export interface LogEntry {
  id: string | number;
  timestamp?: Date;
  message: string;
  level?: 'INFO' | 'WARN' | 'ERROR' | 'DEBUG'; // Optional: for styling
}

interface LogViewerProps {
  logs: Accessor<LogEntry[]>; // Logs should be passed as an accessor (signal/prop)
  isLoading: Accessor<boolean>;
  showTimestamps?: Accessor<boolean>; // Prop to control timestamp visibility
  defaultItemHeight?: number; // For virtualizer
  containerHeight?: string; // e.g., "600px"
  placeholder?: string; // Placeholder when logs are empty
}

export function SolidLogViewer(props: LogViewerProps) {
  const [isSearchActive, setIsSearchActive] = createSignal(false);
  const [searchTerm, setSearchTerm] = createSignal('');
  let parentRef: HTMLDivElement | undefined = undefined;
  let searchInputRef: HTMLInputElement | undefined;

  const defaultShowTimestamps = () => props.showTimestamps !== undefined ? props.showTimestamps() : true;
  const itemHeight = () => props.defaultItemHeight || 22; // Estimated height of a single log line
  const containerHeight = () => props.containerHeight || '500px';
  const placeholderMessage = () => props.placeholder || 'No logs to display.';

  // Filter logs based on search term
  const filteredLogs = createMemo(() => {
    const term = searchTerm().toLowerCase();
    if (!term) {
      return props.logs();
    }
    return props.logs().filter(log => log.message.toLowerCase().includes(term));
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

  const highlightMatch = (text: string, term: string) => {
    if (!term.trim()) {
      return text;
    }
    // Escape special characters in search term for regex
    const escapedTerm = term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const parts = text.split(new RegExp(`(${escapedTerm})`, 'gi'));
    return (
      <For each={parts}>
        {(part) => (
          part.toLowerCase() === term.toLowerCase() ? <strong>{part}</strong> : part
        )}
      </For>
    );
  };

  const formatTimestamp = (date: Date): string => {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', fractionalSecondDigits: 3 });
  };

  return (
    <div class="solid-log-viewer-wrapper">
      <Show when={isSearchActive()}>
        <div class="search-bar">
          <input
            ref={searchInputRef}
            type="text"
            placeholder="Search logs..."
            value={searchTerm()}
            onInput={(e) => setSearchTerm(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.preventDefault(); // Prevent form submission if any
              if (e.key === 'Escape') {
                setIsSearchActive(false);
                setSearchTerm('');
              }
            }}
          />
          <button onClick={() => { setIsSearchActive(false); setSearchTerm(''); }}>×</button>
        </div>
      </Show>

      <Show when={props.isLoading()} fallback={
        <div
          ref={parentRef}
          class="log-container"
          style={{ height: containerHeight(), overflow: 'auto' }} // font-family and font-size moved to CSS
        >
          <Show when={filteredLogs().length > 0} fallback={
             <div class="placeholder">{searchTerm() ? 'No matching logs.' : placeholderMessage()}</div>
          }>
            <div
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
                width: '100%',
                position: 'relative',
              }}
            >
              <For each={rowVirtualizer.getVirtualItems()}>
                {(virtualRow) => {
                  const log = createMemo(() => filteredLogs()[virtualRow.index]);
                  return (
                    <div
                      class="log-item"
                      style={{
                        position: 'absolute',
                        top: '0',
                        left: '0',
                        width: '100%',
                        // height is determined by content & virtualizer measurement
                        transform: `translateY(${virtualRow.start}px)`,
                        // display and align-items moved to CSS
                      }}
                      data-index={virtualRow.index}
                    >
                      <Show when={defaultShowTimestamps() && log()?.timestamp}>
                        <span class="timestamp"> {/* styles moved to CSS */}
                          {log() ? formatTimestamp(log()!.timestamp!) : ''}
                        </span>
                      </Show>
                      <span class="message"> {/* styles moved to CSS */}
                        {log() ? highlightMatch(log()!.message, searchTerm()) : ''}
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </Show>
        </div>
      }>
        <div class="loading-indicator" style={{ height: containerHeight() }}> {/* display, align-items, justify-content moved to CSS */}
          Loading logs...
        </div>
      </Show>
    </div>
  );
}