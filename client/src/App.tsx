import { Router, Route, A } from '@solidjs/router'; // Router is likely a function, Link a component
import SessionDetailPage from './pages/SessionDetailPage';
import SessionListPage from './SessionListPage';
import { AuthGuard, OAuthCallbackPage } from './auth/AuthGuard';

export default function App() {
  return <Router>
    <Route path="/oauth_callback" component={OAuthCallbackPage}/>
    <Route path="/" component={AuthGuard}>
      <Route path="/sessions/:project_id" component={SessionListPage} />
      <Route path="/sessions/:project_id/:session_id" component={SessionDetailPage} />
    </Route>
  </Router>
}
