import { createResource, type Accessor } from 'solid-js';
import { auth_headers } from '../auth/AuthGuard';

export interface SessionMetadata {
  session_id: string;
  timestamp_utc?: string; // Added optional timestamp, as it's used for video sync
  username?: string; // Added optional username for listing related sessions
  [key: string]: any;
}

export interface SessionData {
  metadata: SessionMetadata;
  video_url: string;
  log_url: string;
}

async function fetchSessionData(sessionId: string, projectId: string): Promise<SessionData | null> {
  let url = `/rest/session?project_id=${projectId}&session_id=${sessionId}`;
  console.log(`Fetching session data from ${url}`);
  try {
    const response = await fetch(url, { ...auth_headers });
    if (!response.ok) {
      console.error(`HTTP error! status: ${response.status} for URL ${url}`);
      return null;
    }
    return await response.json();
  } catch (error) {
    console.error(`Failed to fetch session data from ${url}:`, error);
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

export function useSession(sessionId: Accessor<string | undefined>, projectId: Accessor<string | undefined>) {
  const [sessionData] = createResource(
    () => {
      const sId = sessionId();
      const pId = projectId ? projectId() : undefined;
      if (!sId) return undefined; // Return undefined if sessionId is not available
      return { sessionId: sId, projectId: pId }; // Pass as an object to re-trigger if either changes
    },
    async (params) => {
      // params will be undefined if sessionId() was undefined in the source function
      if (!params) return null;
      return fetchSessionData(params.sessionId, params.projectId);
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