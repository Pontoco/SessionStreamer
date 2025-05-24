import { Show, type JSX } from 'solid-js';

interface NotFoundProps {
  when: boolean | undefined | null; // True if the item is "not found", false/null/undefined if found
  fallback?: JSX.Element;
  children: JSX.Element;
}

export function NotFound(props: NotFoundProps): JSX.Element {
  return (
    <Show when={!props.when} fallback={props.fallback || <p>Not found.</p>}>
      {props.children}
    </Show>
  );
}