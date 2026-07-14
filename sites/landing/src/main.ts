import './landing.css';
import { mount } from 'svelte';
import Landing from './Landing.svelte';

const app = mount(Landing, {
  target: document.getElementById('app') as HTMLElement,
});

export default app;
