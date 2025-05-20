import { createSignal } from 'solid-js'
import { render } from 'solid-js/web'

function App() {
  const [count, setCount] = createSignal(0)

  return (
    <div>
      
    </div>
  )
}

render(
  () => <App />,
  document.getElementById('root') as HTMLElement
)

export default App

// Input: page.html
// gathers inputs that are references from the file
// serves them. 
// releally it could all be static *except*
// - jsx. That needs a compile step to convert to normal js. (can rust do it?)
// - solidjs compiler. needs to run babel. :( (tough. needs to at least run through deno)

// ah so we're sorta trapped because we want to use vite to drive the solidjs/jsx, but we need to use our custom server to return templates!
// at the very least the html files are "fake" . we need them to be queried from the server.

