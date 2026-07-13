import { mount } from 'svelte';
import App from './App.svelte';
import './styles/tokens.css';
import './styles/base.css';
import './styles/layout.css';
import './styles/components.css';

const target = document.getElementById('app');

if (!target) {
  throw new Error('OpenQuota mount point was not found');
}

mount(App, { target });
