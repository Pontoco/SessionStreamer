import { createResource, For, Show } from 'solid-js';
import { render } from 'solid-js/web';
import { Router, Route, A } from '@solidjs/router'; // Router is likely a function, Link a component
import type { JSX } from 'solid-js';
import SessionDetailPage from './src/SessionDetailPage';

interface SessionMetadata {
  session_id: string;
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

  const getMetadataKeys = (): string[] => {
    const s = sessions();
    if (s && s.length > 0) {
      const allKeys = new Set<string>();
      s.forEach(session => {
        Object.keys(session).forEach(key => allKeys.add(key));
      });
      const sortedKeys = Array.from(allKeys);
      if (sortedKeys.includes('session_id')) {
        return ['session_id', ...sortedKeys.filter(k => k !== 'session_id').sort()];
      }
      return sortedKeys.sort();
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
              <For each={sessions()}>
                {(session: SessionMetadata) => (
                  <tr>
                    <For each={getMetadataKeys()}>
                      {(key: string) => (
                        <td style={{ padding: '8px', border: '1px solid #ddd' }}>
                          {key === 'session_id' ? (
                            <A href={`/session/${session[key]}`}>{String(session[key])}</A>
                          ) : (
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