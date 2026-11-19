import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/space-grotesk";
import "@fontsource-variable/jetbrains-mono";
import App from "./App";

// Só em desenvolvimento no navegador (?mock): simula o backend para poder ver a interface sem o Tauri.
const boot = import.meta.env.DEV && location.search.includes("mock") ? import("./dev-mock") : Promise.resolve();
boot.then(() => ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
));
