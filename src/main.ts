import './styles/app.css';
import { mount } from 'svelte';

import App from './App.svelte';
import DocsSite from './lib/DocsSite.svelte';
import { isDocsRoute } from './lib/docs/eckyIrGuide';

const target = document.getElementById('app') as HTMLElement;

const app = isDocsRoute(window.location.pathname)
  ? mount(DocsSite, { target })
  : mount(App, { target });

export default app;
