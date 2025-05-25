import { Router, Route, A } from '@solidjs/router'; // Router is likely a function, Link a component
import SessionDetailPage from './pages/SessionDetailPage';
import SessionListPage from './SessionListPage';

export default function App() {
  return <Router>
    <Route path="/sessions/:project_id" component={SessionListPage} />
    <Route path="/sessions/:project_id/:session_id" component={SessionDetailPage} />
  </Router>
}
