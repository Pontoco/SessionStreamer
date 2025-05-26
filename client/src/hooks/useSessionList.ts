import { Accessor, createResource } from 'solid-js';
import { SessionMetadata } from './useSession';
import { auth_headers } from '../auth/AuthGuard';

export function useAllSessions(project_id: Accessor<string>) {
    const [sessions] = createResource(
        () => ({ pid: project_id(), headers: auth_headers() }),
        async (source) => {
            if (!source.pid) return null;
            try {
                const response = await fetch('/rest/list?project_id=' + source.pid, { headers: source.headers });

                if (!response.ok) {
                    console.error(`HTTP error! status: ${response.status} fetching /rest/list`);
                    throw new Error(`HTTP error! status: ${response.status}`);
                }

                const data = await response.json();
                return data as SessionMetadata[]; // Assuming the API returns data matching this structure
            } catch (error) {
                console.error("Failed to fetch all sessions from /rest/list:", error);
                return []; // Return empty array on error to prevent breaking UI
            }
        });
    return sessions;
}