import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import '@fontsource-variable/material-symbols-outlined/full.css';
import { setupMockMode } from '@/lib/mock/setup';
import { initDensity } from '@/lib/density';
// P0-A: capture-phase link interceptor — external links open via the
// openLink pipeline instead of WebView defaults.
import { installLinkInterception } from '@/lib/linkInterceptor';
import App from './App';
import './index.css';

setupMockMode();
installLinkInterception();
// P2-⑧: apply the persisted density before first paint.
initDensity();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
