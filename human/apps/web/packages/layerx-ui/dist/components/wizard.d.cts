import * as React from 'react';
import { PlatformSetting } from '../lib/platform.cjs';

interface WizardStep {
    id: string;
    /** Short label for the progress rail / summary. */
    label: string;
    /** The one decision this screen asks for. */
    title: string;
    description?: string;
    render: () => React.ReactNode;
    /** Gate Continue until the step is valid. */
    canContinue?: () => boolean;
}
interface WizardSummaryItem {
    label: string;
    value: React.ReactNode;
}
/**
 * Multi-step journeys, per platform:
 * - mobile:  full-screen wizard — one decision per screen, progress bar,
 *            back button, pinned Continue at thumb reach
 * - desktop: split pane — form on the left, a live "what will happen"
 *            summary pinned on the right
 */
declare function Wizard({ steps, summary, onComplete, onCancel, completeLabel, summaryTitle, platform, className, }: {
    steps: WizardStep[];
    /** Live summary of the choices so far (desktop right rail). */
    summary?: WizardSummaryItem[];
    onComplete?: () => void;
    onCancel?: () => void;
    completeLabel?: string;
    summaryTitle?: string;
    platform?: PlatformSetting;
    className?: string;
}): React.JSX.Element;

export { Wizard, type WizardStep, type WizardSummaryItem };
