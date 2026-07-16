import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource-variable/inter';
import '@fontsource-variable/geist';
import '@fontsource-variable/material-symbols-outlined/full.css';
import { setupMockMode } from '@/lib/mock/setup';
import App from './App';
import './index.css';

setupMockMode();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
