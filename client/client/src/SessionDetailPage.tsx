import { useParams } from '@solidjs/router';
import { createResource, Show, createSignal, createMemo } from 'solid-js';
import type { JSX } from 'solid-js';
import { SolidLogViewer, type LogEntry } from '../components/SolidLogViewer';

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
          message: match[2] || '', // Ensure message is not undefined
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
                <div style={{ display: 'flex', 'flex-direction': 'column', gap: '10px', 'margin-bottom': '10px' }}>
                  <div style={{ display: 'flex', 'align-items': 'center', gap: '10px' }}>
                    <input
                      type="text"
                      placeholder="Filter logs..."
                      value={filterText()}
                      onInput={(e) => setFilterText(e.currentTarget.value)}
                      style={{ padding: '8px', 'border-radius': '4px', border: '1px solid #ccc', width: '300px' }}
                    />
                    <label style={{ display: 'flex', 'align-items': 'center', gap: '5px', cursor: 'pointer' }}>
                      <input
                        type="checkbox"
                        checked={showTimestamps()}
                        onChange={(e) => setShowTimestamps(e.currentTarget.checked)}
                      />
                      Show Timestamps
                    </label>
                  </div>
                </div>
                <SolidLogViewer
                  logs={processedLogs}
                  isLoading={() => rawLogContent.loading}
                  containerHeight="500px"
                  showTimestamps={showTimestamps}
                />
                 <Show when={!rawLogContent.loading && rawLogContent() === undefined && sessionData()?.log_url}>
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