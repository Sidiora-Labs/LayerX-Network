import * as React from 'react';
import * as SwitchPrimitive from '@radix-ui/react-switch';

/** iOS-style switch used across settings sheets in the design set. */
declare const Switch: React.ForwardRefExoticComponent<Omit<SwitchPrimitive.SwitchProps & React.RefAttributes<HTMLButtonElement>, "ref"> & React.RefAttributes<HTMLButtonElement>>;

export { Switch };
