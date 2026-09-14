import * as React from 'react';

/** Big number over a muted label — "18 / Total referrals". */
declare function Stat({ value, label, className, align, }: {
    value: React.ReactNode;
    label: React.ReactNode;
    className?: string;
    align?: "left" | "center";
}): React.JSX.Element;
/** Two stats separated by a vertical hairline, as on Earning / Network. */
declare function StatPair({ left, right, className, }: {
    left: {
        value: React.ReactNode;
        label: React.ReactNode;
    };
    right: {
        value: React.ReactNode;
        label: React.ReactNode;
    };
    className?: string;
}): React.JSX.Element;

export { Stat, StatPair };
