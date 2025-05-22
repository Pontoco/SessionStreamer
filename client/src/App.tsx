import { Router, Route, A } from '@solidjs/router'; // Router is likely a function, Link a component
import SessionDetailPage from './SessionDetailPage';
import SessionListPage from './SessionListPage';

export default function App() {
  return <Router>
    <Route path="/" component={SessionListPage} />
    <Route path="/session/:session_id" component={SessionDetailPage} />
  </Router>
}
