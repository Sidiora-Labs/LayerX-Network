import React from 'react';
import { useFormContext } from 'react-hook-form';

import type { ContractAbiItemInput } from '../types';

import type { Props as AccordionProps } from './ContractMethodFieldAccordion';
import ContractMethodFieldAccordion from './ContractMethodFieldAccordion';
import ContractMethodFieldInput from './ContractMethodFieldInput';
import ContractMethodFieldInputArray from './ContractMethodFieldInputArray';
import { getFieldLabel, matchArray } from './utils';

interface Props extends Pick<AccordionProps, 'onAddClick' | 'onRemoveClick' | 'index'> {
  data: ContractAbiItemInput;
  basePath: string;
  level: number;
  isDisabled: boolean;
  isOptional?: boolean;
}

const ContractMethodFieldInputTuple = ({ data, basePath, level, isDisabled, isOptional, ...accordionProps }: Props) => {
  const { formState: { errors } } = useFormContext();
  const fieldsWithErrors = Object.keys(errors);
  const isInvalid = fieldsWithErrors.some((field) => field.startsWith(basePath));

  if (!('components' in data)) {
    return null;
  }

  return (
    <ContractMethodFieldAccordion
      { ...accordionProps }
      level={ level }
      label={ getFieldLabel(data) }
      isInvalid={ isInvalid }
    >
      { data.components?.map((component, index) => {
        if ('components' in component && component.type === 'tuple') {
          return (
            <ContractMethodFieldInputTuple
              key={ index }
              data={ component }
              basePath={ `${ basePath }:${ index }` }
              level={ level + 1 }
              isDisabled={ isDisabled }
              isOptional={ isOptional }
            />
          );
        }

        const arrayMatch = matchArray(component.type);
        if (arrayMatch) {
          return (
            <ContractMethodFieldInputArray
              key={ index }
              data={ component }
              basePath={ `${ basePath }:${ index }` }
              level={ arrayMatch.itemType === 'tuple' ? level + 1 : level }
              isDisabled={ isDisabled }
            />
          );
        }

        return (
          <ContractMethodFieldInput
            key={ index }
            data={ component }
            path={ `${ basePath }:${ index }` }
            isDisabled={ isDisabled }
            isOptional={ isOptional }
            level={ level }
          />
        );
      }) }
    </ContractMethodFieldAccordion>
  );
};

export default React.memo(ContractMethodFieldInputTuple);
