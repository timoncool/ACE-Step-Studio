import './src-styles.css';
import { installApiBase } from './services/apiBase';
import { startBridge } from './services/mcpBridge';
import { installExternalLinkHandler } from './services/externalLinks';

// Must run before any component issues a request.
installApiBase();
startBridge();
// In the desktop window an external link must go to the system browser,
// not replace the application.
installExternalLinkHandler();
import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import { AuthProvider } from './context/AuthContext';
import { ResponsiveProvider } from './context/ResponsiveContext';

const rootElement = document.getElementById('root');
if (!rootElement) {
  throw new Error("Could not find root element to mount to");
}

const root = ReactDOM.createRoot(rootElement);
root.render(
  <React.StrictMode>
    <AuthProvider>
      <ResponsiveProvider>
        <App />
      </ResponsiveProvider>
    </AuthProvider>
  </React.StrictMode>
);