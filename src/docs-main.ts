import './styles/app.css';
import { mount } from 'svelte';

import DocsSite from './lib/DocsSite.svelte';

const app = mount(DocsSite, {
  target: document.getElementById('app') as HTMLElement,
});

export default app;
