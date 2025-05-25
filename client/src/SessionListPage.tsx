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
import { A, useSearchParams } from '@solidjs/router';
import { useAllSessions } from './hooks/useSessionList';
import { SessionMetadata } from './hooks/useSession';

export default function SessionListPage(): JSX.Element {
    const [searchParams] = useSearchParams();
    const projectId = createMemo(() => searchParams.project_id as string);
    const sessions = useAllSessions(projectId);
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
      <main class="flex-grow flex flex-col bg-neutral-50 dark:bg-neutral-900 text-neutral-800 dark:text-neutral-200 min-h-screen">
        <Show
            when={projectId() && projectId()!.trim() !== ""}
            fallback={
                <>
                    <div class="flex items-center justify-start gap-4 p-4 border-b border-neutral-200 dark:border-neutral-700">
                        <h1 class="text-lg font-semibold text-red-600 dark:text-red-400">Project ID Required</h1>
                    </div>
                    <div class="p-6">
                        <p class="text-red-600 dark:text-red-400">
                            Error: The <code>project_id</code> query parameter must be specified in the URL.
                        </p>
                        <p class="mt-2 text-sm text-neutral-600 dark:text-neutral-400">
                            Please append it to the URL, for example: <code>?project_id=your_project_id</code>
                        </p>
                    </div>
                </>
            }
        >
            <div class="flex items-center justify-start gap-4 p-4 border-b border-neutral-200 dark:border-neutral-700">
              <h1 class="text-lg font-semibold">All Sessions for Project: {projectId()}</h1>
            </div>
            <div class="p-6 flex-grow">
                <Show when={sessions.loading}>
                    <p class="text-neutral-500 dark:text-neutral-400">Loading sessions...</p>
                </Show>
                <Show when={sessions.error && !sessions.loading}>
                    <p class="text-red-500 dark:text-red-400">
                        Error loading sessions: {sessions.error.message || "An unknown error occurred."}
                    </p>
                </Show>
                <Show when={!sessions.loading && !sessions.error}>
                    <Show when={sessions() && sessions()!.length > 0} 
                          fallback={
                            <p class="text-neutral-500 dark:text-neutral-400">
                                No sessions found for project <code class="font-semibold">{projectId()}</code>.
                            </p>
                          }
                    >
                        <div class="overflow-x-auto bg-white dark:bg-neutral-800 shadow-lg rounded-lg">
                            <table class="min-w-full divide-y divide-neutral-200 dark:divide-neutral-700">
                                <thead class="bg-neutral-50 dark:bg-neutral-750">
                                    <For each={table.getHeaderGroups()}>
                                        {headerGroup => (
                                            <tr>
                                                <For each={headerGroup.headers}>
                                                    {header => (
                                                        <th scope="col" class="px-6 py-3 text-left text-xs font-semibold text-neutral-600 dark:text-neutral-300 uppercase tracking-wider border-b border-neutral-200 dark:border-neutral-700">
                                                            <div
                                                                class={`flex items-center gap-1 ${header.column.getCanSort() ? 'cursor-pointer select-none' : 'select-none'}`}
                                                                onClick={header.column.getToggleSortingHandler()}
                                                            >
                                                                {flexRender(header.column.columnDef.header, header.getContext())}
                                                                <span class="text-neutral-400 dark:text-neutral-500">
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
                                                                        class="block w-full rounded-md border-neutral-300 dark:border-neutral-600 bg-white dark:bg-neutral-700 text-neutral-900 dark:text-neutral-200 shadow-sm focus:border-brand-500 focus:ring-brand-500 sm:text-xs p-1.5"
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
                                <tbody class="divide-y divide-neutral-200 dark:divide-neutral-700">
                                    <For each={table.getRowModel().rows}>
                                        {row => (
                                            <tr class="hover:bg-neutral-100 dark:hover:bg-neutral-700 transition-colors">
                                                <For each={row.getVisibleCells()}>
                                                    {cell => (
                                                        <td class="px-6 py-4 whitespace-nowrap text-sm text-neutral-800 dark:text-neutral-200">
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
        </Show>
      </main>
    );
}
