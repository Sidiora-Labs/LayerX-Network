import * as React from 'react';

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
    error?: boolean;
    /** Optional leading adornment (icon, flag, "+1" etc). */
    leading?: React.ReactNode;
    trailing?: React.ReactNode;
}
declare const Input: React.ForwardRefExoticComponent<InputProps & React.RefAttributes<HTMLInputElement>>;

interface SearchInputProps extends React.InputHTMLAttributes<HTMLInputElement> {
    onClear?: () => void;
}
/** Pill search field, as used in the home header and asset list. */
declare const SearchInput: React.ForwardRefExoticComponent<SearchInputProps & React.RefAttributes<HTMLInputElement>>;

export { Input, type InputProps, SearchInput, type SearchInputProps };
