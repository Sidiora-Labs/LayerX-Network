import * as React from 'react';

interface BankCardData {
    holder: string;
    /** Masked number, e.g. "6464 XXXX XXXX 9980". */
    number: string;
    kind?: string;
    balanceLabel?: string;
    balance?: string;
    expiry?: string;
    brand?: string;
    status?: {
        label: string;
        tone?: "success" | "neutral" | "destructive";
    };
    theme?: "light" | "dark";
}
/** Payment-card visual from the wallet/cards screens. */
declare function BankCard({ data, className }: {
    data: BankCardData;
    className?: string;
}): React.JSX.Element;
/** Horizontal card pager with dot indicator, as on the Cards screen. */
declare function CardCarousel({ cards, renderCard, className, }: {
    cards: BankCardData[];
    renderCard?: (card: BankCardData, index: number) => React.ReactNode;
    className?: string;
}): React.JSX.Element;

export { BankCard, type BankCardData, CardCarousel };
