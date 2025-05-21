import { createResource, For, Show, createMemo } from 'solid-js';
import { render } from 'solid-js/web';
import { Router, Route, A } from '@solidjs/router'; // Router is likely a function, Link a component
import type { JSX } from 'solid-js';
import SessionDetailPage from './src/SessionDetailPage';

interface SessionMetadata {
  session_id: string;
  timestamp: string; // Added for RFC3339 timestamp
  formattedTimestamp?: string; // Added for human-readable version, optional
  [key: string]: any;
}

async function fetchSessions(): Promise<SessionMetadata[]> {
  try {
    const response = await fetch('/rest/list');
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    const data = await response.json();
    return data;
  } catch (error) {
    console.error("Failed to fetch sessions:", error);
    return [];
  }
}

function SessionListPage(): JSX.Element {
  const [sessions] = createResource(fetchSessions);

  // Helper function to format RFC3339 timestamp to a human-readable string
  function formatTimestamp(rfc3339Timestamp?: string): string {
    if (!rfc3339Timestamp) return '';
    try {
      return new Date(rfc3339Timestamp).toLocaleString('en-US', {
        year: 'numeric', month: 'long', day: 'numeric',
        hour: 'numeric', minute: '2-digit', hour12: true
      });
    } catch (e) {
      console.error("Error formatting timestamp:", rfc3339Timestamp, e);
      return 'Invalid Date';
    }
  }

  const processedSessions = createMemo(() => {
    const s = sessions();
    if (!s || s.length === 0) return [];
    
    // Filter for sessions that have a valid timestamp string
    const sessionsWithTimestamp = s.filter(session => typeof session.timestamp === 'string' && session.timestamp.length > 0);
    
    return [...sessionsWithTimestamp] // Create a shallow copy before sorting
      .sort((a, b) => {
        // Sort descending (newest first)
        return b.timestamp.localeCompare(a.timestamp);
      })
      .map(session => ({
        ...session,
        // Add the formatted timestamp
        formattedTimestamp: formatTimestamp(session.timestamp)
      }));
  });

  const getMetadataKeys = (): string[] => {
    const s = sessions(); // Use raw sessions to get original keys
    if (s && s.length > 0) {
      const allKeys = new Set<string>();
      // Collect all unique keys from the raw session data
      s.forEach(session => {
        Object.keys(session).forEach(key => {
          // Exclude our derived 'formattedTimestamp' if it somehow got into raw data
          if (key !== 'formattedTimestamp') {
            allKeys.add(key);
          }
        });
      });

      const baseKeys = Array.from(allKeys);
      let orderedKeys: string[] = [];

      // 1. session_id (if exists)
      if (baseKeys.includes('session_id')) {
        orderedKeys.push('session_id');
      }

      // 2. "Timestamp" (this is the header for our human-friendly session.formattedTimestamp)
      orderedKeys.push('Timestamp');

      // 3. Specific keys in preferred order (e.g., username, raw timestamp)
      //    Ensure 'timestamp' (raw RFC3339) is included as requested.
      const specificKeyOrder = ['username', 'timestamp'];
      specificKeyOrder.forEach(key => {
        if (baseKeys.includes(key) && !orderedKeys.includes(key)) {
          orderedKeys.push(key);
        }
      });

      // 4. Add remaining keys, sorted alphabetically
      baseKeys
        .filter(key => !orderedKeys.includes(key))
        .sort()
        .forEach(key => orderedKeys.push(key));
      
      return orderedKeys;
    }
    return [];
  };

  return (
    <div style={{ padding: '20px' }}>
      <h1>Session List</h1>
      <Show when={!sessions.loading} fallback={<p>Loading sessions...</p>}>
        <Show when={sessions() && sessions()!.length > 0} fallback={<p>No sessions found.</p>}>
          <table style={{ width: '100%', 'border-collapse': 'collapse', border: '1px solid #ddd' }}>
            <thead>
              <tr>
                <For each={getMetadataKeys()}>
                  {(key: string) => <th style={{ padding: '8px', border: '1px solid #ddd', 'background-color': '#f2f2f2' }}>{key}</th>}
                </For>
              </tr>
            </thead>
            <tbody>
              <For each={processedSessions()}>
                {(session: SessionMetadata) => ( // session type now includes formattedTimestamp via SessionMetadata interface update
                  <tr>
                    <For each={getMetadataKeys()}>
                      {(key: string) => (
                        <td style={{ padding: '8px', border: '1px solid #ddd' }}>
                          {key === 'session_id' ? (
                            <A href={`/session/${session.session_id}`}>{String(session.session_id)}</A>
                          ) : key === 'Timestamp' ? ( // Display the formatted timestamp
                            String(session.formattedTimestamp === undefined ? '' : session.formattedTimestamp)
                          ) : ( // Display other keys, including the raw 'timestamp'
                            String(session[key] === undefined ? '' : session[key])
                          )}
                        </td>
                      )}
                    </For>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
      </Show>
    </div>
  );
}

render(
  // If AppRouterComponent is a component function, it needs to be invoked.
  // If it's already a JSX element, this is fine.
  // Given previous errors, it's likely a component function.
  () => (
    <Router>
      <Route path="/" component={SessionListPage} />
      <Route path="/session/:session_id" component={SessionDetailPage} />
    </Router>
  ),
  document.getElementById('root') as HTMLElement
);

// No explicit App export needed if AppRouterComponent is the root.