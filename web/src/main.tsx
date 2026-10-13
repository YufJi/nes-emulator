import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
// Must be imported before any component renders: it ships the fonts and all
// island styles. Our own sheet comes after it so local rules can layer on top.
import 'animal-island-ui/style';
import App from './App';
import './styles.css';
import { enterPhoneLayoutOnRoot } from './mobile/entry';
import { restoreRedirectedRoute } from './router';

const container = document.getElementById('root');
if (!container) {
  throw new Error('页面缺少 #root 挂载点。');
}

// A deep link served through `404.html` carries its location in `?route=`; put
// it back before anything reads the address bar.
restoreRedirectedRoute();

// A phone opening the root gets the phone layout, decided before the first
// render so the wide-screen page never flashes.
enterPhoneLayoutOnRoot();

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
