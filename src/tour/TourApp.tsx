import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronLeftIcon, ChevronRightIcon } from "../components/Pill/ui/icons";
import { t, type MessageKey } from "../lib/i18n";
import { postToHost } from "./host";
import { buildSteps, TOUR_STEP_COUNT } from "./steps";
import { TourStage } from "./TourStage";

/** How long autoplay stays on a step. */
export const AUTOPLAY_MS = 5000;

interface TourAppProps {
  /** 0-based first step. */
  initialStep?: number;
  autoplay?: boolean;
  autoplayMs?: number;
}

const titleKey = (index: number) => `tour.s${index + 1}.title` as MessageKey;
const textKey = (index: number) => `tour.s${index + 1}.text` as MessageKey;

function PlayIcon({ paused }: { paused: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" focusable="false">
      {paused ? <path d="M7 4.8v14.4a1 1 0 0 0 1.5.86l12-7.2a1 1 0 0 0 0-1.72l-12-7.2A1 1 0 0 0 7 4.8Z" /> : <path d="M7 4h3.5v16H7zM13.5 4H17v16h-3.5z" />}
    </svg>
  );
}

/**
 * The tour page: an explanation, a mock island that morphs through twelve scenes, and the
 * controls. Autoplay is on until the person navigates by hand. Nothing here talks to the island's
 * backend; the only messages go to the Island Center that shows the page (host.ts).
 */
export function TourApp({ initialStep = 0, autoplay: initialAutoplay = true, autoplayMs = AUTOPLAY_MS }: TourAppProps) {
  const steps = useMemo(buildSteps, []);
  const last = TOUR_STEP_COUNT - 1;
  const [step, setStep] = useState(() => Math.min(last, Math.max(0, initialStep)));
  const [autoplay, setAutoplay] = useState(initialAutoplay && initialStep < last);
  const current = steps[step];

  // Autoplay: one timer for the step on screen, none once it is off or at the last step.
  useEffect(() => {
    if (!autoplay || step >= last) return;
    const timer = setTimeout(() => {
      setStep(step + 1);
      if (step + 1 >= last) setAutoplay(false);
    }, autoplayMs);
    return () => clearTimeout(timer);
  }, [autoplay, step, last, autoplayMs]);

  // Navigating by hand ends autoplay.
  const go = useCallback(
    (index: number) => {
      setAutoplay(false);
      setStep(Math.min(last, Math.max(0, index)));
    },
    [last]
  );

  // The page is right to left: Left is forward, Right is back.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey) return;
      if (e.key === "ArrowLeft") go(step + 1);
      else if (e.key === "ArrowRight") go(step - 1);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [go, step]);

  const toggleAutoplay = () => {
    if (autoplay) {
      setAutoplay(false);
      return;
    }
    if (step >= last) setStep(0);
    setAutoplay(true);
  };

  const isLast = step === last;

  return (
    <main className="tour-root" dir="rtl" aria-label={t("tour.pageTitle")}>
      <div className="tour-layout">
        {/* Announced only while the person moves by hand: autoplay would queue a new announcement every few seconds. */}
        <p className="sr-only" role="status" aria-live="polite" aria-atomic="true">
          {autoplay ? "" : `${t("tour.step", { n: step + 1, total: TOUR_STEP_COUNT })}: ${t(titleKey(step))}`}
        </p>
        <section className="tour-copy">
          <div key={step} className="tour-copy__inner">
            <p className="tour-eyebrow">{t("tour.step", { n: step + 1, total: TOUR_STEP_COUNT })}</p>
            <h1 className="tour-title">{t(titleKey(step))}</h1>
            <p className="tour-text">{t(textKey(step))}</p>
            {current.action && (
              <button
                type="button"
                className="tour-btn tour-btn--ghost tour-action"
                onClick={() => postToHost({ type: "navigate", page: current.action!.page })}
              >
                {t(current.action.labelKey)}
              </button>
            )}
          </div>
        </section>

        <div className="tour-stage-wrap">
          <TourStage size={current.size} layer={current.layer}>
            {current.content}
          </TourStage>
        </div>
      </div>

      <nav className="tour-controls" aria-label={t("tour.dots")}>
        <button type="button" className="tour-btn tour-btn--secondary" disabled={step === 0} onClick={() => go(step - 1)}>
          <ChevronRightIcon size={16} strokeWidth={2.4} />
          {t("tour.prev")}
        </button>

        <ol className="tour-dots">
          {steps.map((candidate, index) => (
            <li key={candidate.id}>
              <button
                type="button"
                className="tour-dot"
                data-state={index < step ? "past" : index === step ? "current" : "future"}
                aria-label={t("tour.dot", { n: index + 1, title: t(titleKey(index)) })}
                aria-current={index === step ? "step" : undefined}
                onClick={() => go(index)}
              >
                <span className="tour-dot__pip">
                  {index === step && (
                    <span key={step} className={`tour-dot__fill${autoplay ? " tour-dot__fill--run" : ""}`} style={{ animationDuration: `${autoplayMs}ms` }} />
                  )}
                </span>
              </button>
            </li>
          ))}
        </ol>

        {isLast ? (
          <button key="finish" type="button" className="tour-btn tour-btn--primary" onClick={() => postToHost({ type: "done" })}>
            {t("tour.finish")}
          </button>
        ) : (
          <button key="next" type="button" className="tour-btn tour-btn--primary" onClick={() => go(step + 1)}>
            {t("tour.next")}
            <ChevronLeftIcon size={16} strokeWidth={2.4} />
          </button>
        )}

        <button type="button" className="tour-autoplay" aria-pressed={autoplay} onClick={toggleAutoplay}>
          <PlayIcon paused={!autoplay} />
          {t("tour.autoplay")}
        </button>
      </nav>
    </main>
  );
}
