import React from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { normalizePath, SitePage } from "./pages";
import "./website.css";

const pathname = normalizePath(window.location.pathname);

const root = document.getElementById("root")!;
const app = <React.StrictMode><SitePage pathname={pathname} /></React.StrictMode>;

if (import.meta.env.DEV) createRoot(root).render(app);
else hydrateRoot(root, app);
