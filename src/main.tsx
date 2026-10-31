import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./app/App";
import { markStartup } from "./lib/startup";
import "./styles/theme.css";
import "./styles/desktop-polish.css";
import "./styles/launcher-presentation.css";
import "./styles/launcher-chrome.css";

markStartup("app_init");
markStartup("frontend_boot");
createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
