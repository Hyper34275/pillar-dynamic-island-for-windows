import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import { applyDocumentLocale, setFixedLocale, setFormatLocale } from "../lib/i18n";
import { Gallery } from "./Gallery";

// Design QA only (npm run dev → /gallery.html): every island presentation at its real size, built
// from the real components and the tour's made-up data. Not part of any build.
const params = new URLSearchParams(window.location.search);
setFixedLocale("he");
applyDocumentLocale();
setFormatLocale(params.get("format") || "he-IL");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Gallery only={params.get("only")} exhibit={params.get("exhibit")} />
  </React.StrictMode>
);
