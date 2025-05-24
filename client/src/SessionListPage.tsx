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
    timestamp: string;
    formattedTimestamp?: string;
    username?: string;
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

        return [...s]
            .sort((a, b) => {
                if (!a.timestamp && !b.timestamp) return 0;
                if (!a.timestamp) return 1;
                if (!b.timestamp) return -1;

                return b.timestamp.localeCompare(a.timestamp);
            })
            .map(session => ({
                ...session,
                formattedTimestamp: formatTimestamp(session.timestamp)
            }));
    });

    const columns = createMemo<ColumnDef<SessionMetadata>[]>(() => [
        {
            accessorKey: 'session_id',
            header: 'Session ID',
            cell: info => <A class="text-brand-700 hover:text-brand-600 hover:underline font-medium" href={`/session/${info.getValue()}`}>{String(info.getValue())}</A>,
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
    });

    return (
        <div class="max-w-7xl mx-auto">
            <h1 class="text-heading-2 mb-6 px-6 sm:px-8 pt-6 sm:pt-8">Session List</h1>
            <Show when={!sessions.loading} fallback={<p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">Loading sessions...</p>}>
                <Show when={sessions() && sessions()!.length > 0} fallback={<p class="text-neutral-500 px-6 sm:px-8 py-6 sm:py-8">No sessions found.</p>}>
                    <div class="overflow-x-auto bg-white shadow-lg rounded-lg p-6 sm:p-8">
                        <table class="min-w-full divide-y divide-neutral-200">
                            <thead class="">
                                <For each={table.getHeaderGroups()}>
                                    {headerGroup => (
                                        <tr>
                                            <For each={headerGroup.headers}>
                                                {header => (
                                                    <th scope="col" class="px-6 py-3 text-left text-xs font-semibold text-neutral-600 uppercase tracking-wider border-b border-neutral-200">
                                                        <div
                                                            class={`flex items-center gap-1 ${header.column.getCanSort() ? 'cursor-pointer select-none' : 'select-none'}`}
                                                            onClick={header.column.getToggleSortingHandler()}
                                                        >
                                                            {flexRender(header.column.columnDef.header, header.getContext())}
                                                            <span class="text-neutral-400">
                                                            {{
                                                                asc: '↑',
                                                                desc: '↓',
                                                            }[header.column.getIsSorted() as string] ?? ''}
                                                            </span>
                                                        </div>
                                                        {header.column.getCanFilter() ? (
                                                            <div class="mt-1.5">
                                                                <input
                                                                    type="text"
                                                                    value={(header.column.getFilterValue() ?? '') as string}
                                                                    onInput={e => header.column.setFilterValue(e.currentTarget.value)}
                                                                    placeholder={`Filter...`}
                                                                    class="block w-full rounded-md border-neutral-300 shadow-sm focus:border-brand-500 focus:ring-brand-500 sm:text-xs p-1.5"
                                                                    onClick={e => e.stopPropagation()}
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
                            <tbody class="divide-y divide-neutral-200">
                                <For each={table.getRowModel().rows}>
                                    {row => (
                                        <tr class="hover:bg-neutral-100 transition-colors">
                                            <For each={row.getVisibleCells()}>
                                                {cell => (
                                                    <td class="px-6 py-4 whitespace-nowrap text-sm text-neutral-800">
                                                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                                                    </td>
                                                )}
                                            </For>
                                        </tr>
                                    )}
                                </For>
                            </tbody>
                        </table>
                    </div>
                </Show>
            </Show>
        </div>
    );
}
