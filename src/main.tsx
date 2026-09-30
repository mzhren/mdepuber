import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import "./styles.css";
import { applyTheme, resolveInitialTheme } from "./lib/theme";

// Apply the stored/system theme before the first paint so the window never
// flashes the wrong colour scheme.
applyTheme(resolveInitialTheme());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
