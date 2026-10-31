import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App.js";
import "./styles/globals.css";

const root = document.getElementById("root");
if (root === null) {
  throw new Error("Missing #root element");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
