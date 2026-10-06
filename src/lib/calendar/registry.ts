import { createClassicOutlookProvider } from "./classicOutlook";
import type { CalendarProvider, CalendarProviderFactory } from "./provider";

const factories = new Map<string, CalendarProviderFactory>();

/** Registers (or replaces) a provider factory. See provider.ts for how to add a source. */
export function registerCalendarProvider(id: string, factory: CalendarProviderFactory): void {
  factories.set(id, factory);
}

export function registerDefaultProviders(): void {
  registerCalendarProvider("classic-outlook", () => createClassicOutlookProvider());
}

/** Starts every registered provider. */
export function createRegisteredProviders(): CalendarProvider[] {
  return [...factories.values()].map((factory) => factory());
}
