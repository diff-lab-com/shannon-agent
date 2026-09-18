import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import '@fontsource-variable/material-symbols-outlined/full.css';
import { setupMockMode } from '@/lib/mock/setup';
import { initDensity } from '@/lib/density';
import App from './App';
import './index.css';

setupMockMode();
// P2-⑧: apply the persisted density before first paint.
initDensity();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
