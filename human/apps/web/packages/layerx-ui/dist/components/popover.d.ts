import * as React from 'react';
import * as PopoverPrimitive from '@radix-ui/react-popover';

declare const Popover: React.FC<PopoverPrimitive.PopoverProps>;
declare const PopoverTrigger: React.ForwardRefExoticComponent<PopoverPrimitive.PopoverTriggerProps & React.RefAttributes<HTMLButtonElement>>;
/** Anchored popover surface — desktop filter chips, bell menu, etc. */
declare const PopoverContent: React.ForwardRefExoticComponent<Omit<PopoverPrimitive.PopoverContentProps & React.RefAttributes<HTMLDivElement>, "ref"> & React.RefAttributes<HTMLDivElement>>;

export { Popover, PopoverContent, PopoverTrigger };
