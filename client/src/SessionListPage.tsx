import type { Component } from 'solid-js';

import { createResource, For, Show, createMemo, createSignal } from 'solid-js';
import {
  createSolidTable,
  getCoreRowModel,
  getSortedRowModel,
  getFilteredRowModel,
  flexRender,
} from '@tanstack/solid-table';
import type {
  ColumnDef,
  SortingState,
  ColumnFiltersState,
} from '@tanstack/solid-table';
import type { JSX } from 'solid-js';
import { A } from '@solidjs/router';

interface SessionMetadata {
    session_id: string;
    timestamp: string; // Added for RFC3339 timestamp
    formattedTimestamp?: string; // Added for human-readable version, optional
    username?: string; // Added for clarity, though [key: string] would cover it
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

export default function SessionListPage(): JSX.Element {
    const [sessions] = createResource(fetchSessions);
    const [sorting, setSorting] = createSignal<SortingState>([]);
    const [columnFilters, setColumnFilters] = createSignal<ColumnFiltersState>([]);

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

        return [...s] // Create a shallow copy before sorting
            .sort((a, b) => {
                // Sort descending (newest first)
                if (!a.timestamp && !b.timestamp) return 0;
                if (!a.timestamp) return 1; // b comes first
                if (!b.timestamp) return -1; // a comes first

                return b.timestamp.localeCompare(a.timestamp);
            })
            .map(session => ({
                ...session,
                // Add the formatted timestamp
                formattedTimestamp: formatTimestamp(session.timestamp)
            }));
    });

    const columns = createMemo<ColumnDef<SessionMetadata>[]>(() => [
        {
            accessorKey: 'session_id',
            header: 'Session ID',
            cell: info => <A class="text-blue-600 hover:text-blue-800 hover:underline" href={`/session/${info.getValue()}`}>{String(info.getValue())}</A>,
            enableSorting: true,
            enableColumnFilter: true,
        },
        {
            accessorKey: 'formattedTimestamp',
            header: 'Timestamp',
            cell: info => String(info.getValue() ?? ''),
            enableSorting: true,
            enableColumnFilter: true,
        },
        {
            accessorKey: 'username',
            header: 'Username',
            cell: info => String(info.getValue() ?? ''),
            enableSorting: true,
            enableColumnFilter: true,
        },
        {
            accessorKey: 'timestamp',
            header: 'Raw Timestamp',
            cell: info => String(info.getValue() ?? ''),
            enableSorting: true,
            enableColumnFilter: true,
        },
    ]);

    const table = createSolidTable({
        get data() { return processedSessions(); },
        get columns() { return columns(); },
        state: {
            get sorting() { return sorting(); },
            get columnFilters() { return columnFilters(); },
        },
        onSortingChange: setSorting,
        onColumnFiltersChange: setColumnFilters,
        getCoreRowModel: getCoreRowModel(),
        getSortedRowModel: getSortedRowModel(),
        getFilteredRowModel: getFilteredRowModel(),
        // debugTable: true, // Uncomment for debugging
    });

    return (
        <div class="p-5">
            <h1 class="text-2xl font-bold mb-4">Session List</h1>
            <Show when={!sessions.loading} fallback={<p>Loading sessions...</p>}>
                <Show when={sessions() && sessions()!.length > 0} fallback={<p>No sessions found.</p>}>
                    <table class="w-full border-collapse border border-gray-300">
                        <thead>
                            <For each={table.getHeaderGroups()}>
                                {headerGroup => (
                                    <tr>
                                        <For each={headerGroup.headers}>
                                            {header => (
                                                <th class="p-2 border border-gray-300 bg-gray-100 align-top">
                                                    <div
                                                        class={header.column.getCanSort() ? 'cursor-pointer select-none' : 'select-none'}
                                                        onClick={header.column.getToggleSortingHandler()}
                                                    >
                                                        {flexRender(header.column.columnDef.header, header.getContext())}
                                                        {{
                                                            asc: ' 🔼',
                                                            desc: ' 🔽',
                                                        }[header.column.getIsSorted() as string] ?? ''}
                                                    </div>
                                                    {header.column.getCanFilter() ? (
                                                        <div class="mt-1">
                                                            <input
                                                                type="text"
                                                                value={(header.column.getFilterValue() ?? '') as string}
                                                                onInput={e => header.column.setFilterValue(e.currentTarget.value)}
                                                                placeholder={`Filter...`}
                                                                class="w-[calc(100%-10px)] p-[2px_4px] box-border border border-gray-300 rounded-sm text-sm"
                                                                onClick={e => e.stopPropagation()} // Prevent sort when clicking filter input
                                                            />
                                                        </div>
                                                    ) : null}
                                                </th>
                                            )}
                                        </For>
                                    </tr>
                                )}
                            </For>
                        </thead>
                        <tbody>
                            <For each={table.getRowModel().rows}>
                                {row => (
                                    <tr>
                                        <For each={row.getVisibleCells()}>
                                            {cell => (
                                                <td class="p-2 border border-gray-300">
                                                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
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
