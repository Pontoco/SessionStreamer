import { render } from 'solid-js/web'
import { For } from 'solid-js'
import './session.css'

function SessionPage() {
  // Placeholder for video source and log messages
  // In a real app, these would likely be signals or come from props/context
  const videoSrc = "https://www.w3schools.com/html/mov_bbb.mp4"; // Example video
  const logs = [
    "Log: Session initialized.",
    "Log: Attempting to connect to media server...",
    "Log: Video stream incoming.",
    "Log: Another log message to make it scroll.",
    "Log: And one more for good measure.",
    "Log: This is a very long log message that should wrap nicely within the confines of the log viewer pane, demonstrating how text overflow and wrapping are handled."
  ];

  return (
    <div class="session-container">
      {/* <div class="video-pane">
        <h2>Video Player</h2>
        <video controls src={videoSrc} class="video-player"></video>
      </div>
      <div class="log-pane">
        <h2>Event Logs</h2>
        <div class="log-viewer">
          <For each={logs}>{(log) => <p>{log}</p>}</For>
        </div>
      </div> */}
    </div>
  )
}

render(() => <SessionPage />, document.getElementById('root')!)
