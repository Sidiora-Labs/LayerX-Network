import { get } from 'es-toolkit/compat';
import React from 'react';
import { useFormContext } from 'react-hook-form';

import type { FormFields } from '../types';

import delay from 'lib/delay';
import useFetch from 'lib/hooks/useFetch';
import { FormFieldUrl } from 'toolkit/components/forms/fields/FormFieldUrl';

import ContractVerificationFormRow from '../ContractVerificationFormRow';
import { getGitHubOwnerAndRepo } from '../utils';

interface Props {
  onCommitHashChange: (commitHash?: string) => void;
}

const ContractVerificationFieldGitHubRepo = ({ onCommitHashChange }: Props) => {
  const repoErrorRef = React.useRef<string | undefined>(undefined);
  const fetch = useFetch();
  const { getValues, trigger, getFieldState } = useFormContext<FormFields>();

  const handleBlur = React.useCallback(async() => {
    await delay(100); // have to wait to properly trigger subsequent validation
    const repositoryUrl = getValues('repository_url');
    const { error } = getFieldState('repository_url');

    if (error && error.type !== 'repoUrl') {
      return;
    }

    if (!repositoryUrl) {
      repoErrorRef.current = undefined;
      trigger('repository_url');
      onCommitHashChange();
      return;
    }

    const gitHubData = getGitHubOwnerAndRepo(repositoryUrl);

    if (gitHubData && gitHubData.rest.length === 0 && !gitHubData.url.search && !gitHubData.url.hash) {
      try {
        const response = await fetch(`https://api.github.com/repos/${ gitHubData.owner }/${ gitHubData.repo }/commits?per_page=1`);
        repoErrorRef.current = undefined;
        trigger('repository_url');
        onCommitHashChange(get(response, '[0].sha'));
        return;
      } catch (error) {
        repoErrorRef.current = 'GitHub repository not found';
      }
    } else {
      repoErrorRef.current = 'Invalid GitHub repository URL';
    }

    trigger('repository_url');
    onCommitHashChange();
  }, [ fetch, getValues, getFieldState, onCommitHashChange, trigger ]);

  const repoUrlValidator = React.useCallback(() => {
    return repoErrorRef.current ? repoErrorRef.current : true;
  }, []);

  const rules = React.useMemo(() => {
    return {
      validate: {
        repoUrl: repoUrlValidator,
      },
    };
  }, [ repoUrlValidator ]);

  return (
    <ContractVerificationFormRow>
      <FormFieldUrl<FormFields>
        name="repository_url"
        placeholder="GitHub repository URL"
        required
        onBlur={ handleBlur }
        rules={ rules }
      />
    </ContractVerificationFormRow>
  );
};

export default React.memo(ContractVerificationFieldGitHubRepo);
