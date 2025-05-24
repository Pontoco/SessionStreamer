import { createResource } from 'solid-js';
import { SessionMetadata } from './useSession';

async function fetchAllSessions(): Promise<SessionMetadata[]> {
    try {
        const response = await fetch('/rest/list');
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
}

export function useAllSessions() {
    const [sessions] = createResource(fetchAllSessions);
    return sessions;
}