import * as React from 'react';

declare function Spinner({ className }: {
    className?: string;
}): React.JSX.Element;
declare function Skeleton({ className }: {
    className?: string;
}): React.JSX.Element;
/** List-row shaped skeleton for loading lists. */
declare function SkeletonRow(): React.JSX.Element;

export { Skeleton, SkeletonRow, Spinner };
