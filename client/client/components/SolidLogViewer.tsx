import { createSignal, createEffect, onCleanup, For, Show, createMemo, Accessor } from 'solid-js';
import { createVirtualizer } from '@tanstack/solid-virtual';

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
  let parentRef: HTMLDivElement | undefined;
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

  const rowVirtualizer = createVirtualizer<HTMLDivElement, Element>({
    count: () => filteredLogs().length,
    getScrollElement: () => parentRef,
    estimateSize: (_index) => itemHeight(), // Each item has the same estimated height
    overscan: 10, // Render more items for smoother scrolling
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
          style={{ height: containerHeight(), overflow: 'auto', "font-family": "monospace", "font-size": "14px" }}
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
                        height: `${virtualRow.size}px`,
                        transform: `translateY(${virtualRow.start}px)`,
                        display: 'flex',
                        "align-items": "flex-start", // Align items to the start for multi-line messages
                        "line-height": `${itemHeight() * 0.9}px`, // Adjust line height based on item height
                        "white-space": "pre-wrap", // Allow wrapping for long messages
                        "overflow-wrap": "break-word", // Break long words
                      }}
                      data-index={virtualRow.index}
                    >
                      <Show when={defaultShowTimestamps() && log()?.timestamp}>
                        <span class="timestamp" style={{ "margin-right": "10px", color: "#888" }}>
                          {log() ? formatTimestamp(log()!.timestamp!) : ''}
                        </span>
                      </Show>
                      <span class="message" style={{ "flex-grow": 1 }}>
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
        <div class="loading-indicator" style={{ height: containerHeight(), display: 'flex', "align-items": "center", "justify-content": "center" }}>
          Loading logs...
        </div>
      </Show>

      {/* Basic Styling (consider moving to a separate CSS file) */}
      <style>{`
        .solid-log-viewer-wrapper {
          border: 1px solid #ccc;
          border-radius: 4px;
        }
        .search-bar {
          display: flex;
          padding: 8px;
          border-bottom: 1px solid #eee;
        }
        .search-bar input {
          flex-grow: 1;
          padding: 6px;
          border: 1px solid #ddd;
          border-radius: 3px;
        }
        .search-bar button {
          margin-left: 8px;
          padding: 6px 10px;
          border: 1px solid #ddd;
          background-color: #f0f0f0;
          cursor: pointer;
          border-radius: 3px;
        }
        .search-bar button:hover {
          background-color: #e0e0e0;
        }
        .log-container {
          position: relative; /* Needed for virtualizer positioning */
        }
        .log-item {
          padding: 0 8px; /* Small padding for each item */
          box-sizing: border-box;
        }
        .log-item:hover {
          background-color: #f9f9f9;
        }
        .placeholder, .loading-indicator {
          display: flex;
          justify-content: center;
          align-items: center;
          height: 100%;
          color: #777;
          font-family: sans-serif;
        }
      `}</style>
    </div>
  );
}