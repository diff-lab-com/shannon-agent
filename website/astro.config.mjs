import { defineConfig } from 'astro/config';
import react from '@astrojs/react';

// deploy-website.yml publishes this repo's Pages site at
// https://diff-lab-com.github.io/shannon-agent/ — project pages need the
// /shannon-agent base. Local dev (`pnpm dev`, CI unset) uses the root.
const isCI = process.env.CI === 'true';

export default defineConfig({
  site: 'https://diff-lab-com.github.io',
  base: isCI ? '/shannon-agent' : '/',
  integrations: [react()],
  build: {
    format: 'directory',
  },
});
