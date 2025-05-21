import { useParams } from '@solidjs/router';
import { createResource, Show, onMount, createSignal } from 'solid-js';
import type { JSX } from 'solid-js';

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
  const [sessionData] = createResource(() => params.sessionId, fetchSessionData);
  const [logContent] = createResource(() => sessionData()?.log_url, fetchLogContent);

  return (
    <div style={{ padding: '20px' }}>
      <Show when={sessionData.loading}>
        <p>Loading session details...</p>
      </Show>
      <Show when={!sessionData.loading && sessionData()}>
        {(data) => (
          <div>
            <h1>Session: {data().metadata.session_id}</h1>
            <div style={{ display: 'flex', 'flex-wrap': 'wrap', gap: '20px' }}>
              <div style={{ flex: '1 1 600px', 'min-width': '300px' }}>
                <h2>Video</h2>
                <video controls width="100%" src={data().video_url}>
                  Your browser does not support the video tag.
                </video>
              </div>
              <div style={{ flex: '1 1 400px', 'min-width': '300px' }}>
                <h2>Unity Log</h2>
                <Show when={logContent.loading}>
                  <p>Loading log...</p>
                </Show>
                <Show when={!logContent.loading && logContent() !== undefined}>
                  <pre
                    style={{
                      'max-height': '500px',
                      overflow: 'auto',
                      border: '1px solid #ccc',
                      padding: '10px',
                      'white-space': 'pre-wrap', // Ensure lines wrap
                      'word-break': 'break-all', // Ensure long unbroken strings wrap
                    }}
                  >
                    {logContent()}
                  </pre>
                </Show>
                 <Show when={!logContent.loading && logContent() === undefined && sessionData()?.log_url}>
                    <p>Log file specified but could not be loaded.</p>
                 </Show>
              </div>
            </div>
            <h2 style={{ 'margin-top': '30px' }}>Metadata</h2>
            <pre style={{ border: '1px solid #ccc', padding: '10px', 'background-color': '#f9f9f9' }}>
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