import './styles.css';
import App from './App.svelte';
import { mount } from 'svelte';

const target = document.getElementById('app');

if (!target) {
  throw new Error('Elemento raiz #app não encontrado');
}

mount(App, { target });
