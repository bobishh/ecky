import '../../landing/src/landing.css';
import './hub.css';
import { mount } from 'svelte';
import Hub from './Hub.svelte';
mount(Hub,{target:document.getElementById('app') as HTMLElement});
