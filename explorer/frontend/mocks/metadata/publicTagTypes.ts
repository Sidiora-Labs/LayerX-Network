export const publicTagTypes = {
  tagTypes: [
    {
      id: '96f9db76-02fc-477d-a003-640a0c5e7e15',
      type: 'name' as const,
      description: 'Alias for the address',
    },
    {
      id: 'e75f396e-f52a-44c9-8790-a1dbae496b72',
      type: 'generic' as const,
      description: 'Group classification for the address',
    },
    {
      id: '11a2d4f3-412e-4eb7-b663-86c6f48cdec3',
      type: 'information' as const,
      description: 'Tags with custom data for the address, e.g. additional link to project, or classification details, or minor account details',
    },
    {
      id: 'd37443d4-748f-4314-a4a0-283b666e9f29',
      type: 'classifier' as const,
      description: 'E.g. "ERC20", "Contract", "CEX", "DEX", "NFT"',
    },
    {
      id: 'ea9d0f91-9b46-44ff-be70-128bac468f6f',
      type: 'protocol' as const,
      description: 'Special tag type for protocol-related contracts, e.g. for bridges',
    },
    {
      id: 'd2600acb-473c-445f-ac72-ed6fef53e06a',
      type: 'note' as const,
      description: 'Short general-purpose description for the address',
    },
  ],
};
