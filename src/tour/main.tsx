import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import "./tour.css";
import { applyDocumentLocale, setFixedLocale, setFormatLocale } from "../lib/i18n";
import { parseTourParams } from "./params";
import { TourApp } from "./TourApp";

// The tour is Hebrew, like the island. It cannot ask Windows for the regional format (it never
// talks to the backend), so numbers and the clock follow Israel's unless the address says
// otherwise (tour.html?format=en-US).
const { initialStep, autoplay, format } = parseTourParams(window.location.search);
setFixedLocale("he");
applyDocumentLocale();
setFormatLocale(format);

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <TourApp initialStep={initialStep} autoplay={autoplay} />
  </React.StrictMode>
);
