import { createResource, type Accessor } from 'solid-js';

export interface SessionMetadata {
  session_id: string;
  timestamp?: string; // Added optional timestamp, as it's used for video sync
  username?: string; // Added optional username for listing related sessions
  [key: string]: any;
}

export interface SessionData {
  metadata: SessionMetadata;
  video_url: string;
  log_url: string;
}

async function fetchSessionData(sessionId: string): Promise<SessionData | null> {
  console.log(`Fetching session data for ${sessionId}`);
  try {
    const response = await fetch(`/rest/session/${sessionId}`);
    if (!response.ok) {
      console.error(`HTTP error! status: ${response.status} for session ${sessionId}`);
      return null;
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch session data for ${sessionId}:`, error);
    return null;
  }
}

async function fetchLogContent(logUrl: string): Promise<string | null> {
  if (!logUrl) {
    console.warn("fetchLogContent called with no logUrl");
    return null;
  }
  console.log(`Fetching log content from ${logUrl}`);
  try {
    const response = await fetch(logUrl);
    if (!response.ok) {
      console.error(`HTTP error! status: ${response.status} for log ${logUrl}`);
      return null;
    }
    return await response.text();
  } catch (error) {
    console.error(`Failed to fetch log content from ${logUrl}:`, error);
    return null;
  }
}

export function useSession(sessionId: Accessor<string | undefined>) {
  const [sessionData] = createResource(
    sessionId,
    async (id) => {
      if (!id) return null;
      return fetchSessionData(id);
    }
  );

  const [rawLogContent] = createResource(
    () => {
      const data = sessionData();
      // Only fetch if sessionData is loaded and has a log_url
      if (data && data.log_url) {
        return data.log_url;
      }
      return undefined; // Important: return undefined if not ready to fetch
    },
    async (url) => {
      // fetchLogContent will only be called if url is a string (due to createResource behavior with undefined key)
      return fetchLogContent(url);
    }
  );

  return { sessionData, rawLogContent };
}