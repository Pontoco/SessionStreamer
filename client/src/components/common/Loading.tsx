import { Show, type JSX } from 'solid-js';

interface LoadingProps {
  when: boolean | undefined | null; // True if loading, false/null/undefined if not
  fallback?: JSX.Element;
  children: JSX.Element;
}

export function Loading(props: LoadingProps): JSX.Element {
  return (
    <Show when={!props.when} fallback={props.fallback || <p>Loading...</p>}>
      {props.children}
    </Show>
  );
}