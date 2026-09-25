import React from "react";
import ReactDOM from "react-dom/client";
import { MotionConfig } from "motion/react";

import App from "./App";
import "./index.css";

// "user" follows the system's reduce-motion setting for every animated panel.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <MotionConfig reducedMotion="user">
      <App />
    </MotionConfig>
  </React.StrictMode>,
);
