import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
// Must be imported before any component renders: it ships the fonts and all
// island styles. Our own sheet comes after it so local rules can layer on top.
import 'animal-island-ui/style';
import App from './App';
import './styles.css';

const container = document.getElementById('root');
if (!container) {
  throw new Error('页面缺少 #root 挂载点。');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
