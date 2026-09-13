import * as React from 'react';
import { DateRange } from 'react-day-picker';
export { DateRange } from 'react-day-picker';

/** Shared DayPicker styling (structure via classNames, selection via modifiers). */
declare const dayPickerClassNames: {
    root: string;
    months: string;
    month_caption: string;
    caption_label: string;
    nav: string;
    button_previous: string;
    button_next: string;
    weekdays: string;
    weekday: string;
    week: string;
    day: string;
    day_button: string;
    outside: string;
    disabled: string;
};
declare const dayPickerModifiersClassNames: {
    today: string;
    selected: string;
    range_start: string;
    range_end: string;
    range_middle: string;
};
/**
 * Calendar range picker in an anchored popover — desktop companion to the
 * "From date / To date" selects in the mobile filter sheet.
 */
declare function CalendarRangePicker({ value, onChange, placeholder, className, }: {
    value?: DateRange;
    onChange?: (range: DateRange | undefined) => void;
    placeholder?: string;
    className?: string;
}): React.JSX.Element;

export { CalendarRangePicker, dayPickerClassNames, dayPickerModifiersClassNames };
