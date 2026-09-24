const styles = {
  '.address-entity': {
    '&.address-entity_highlighted': {
      _before: {
        content: `" "`,
        position: 'absolute',
        py: 1,
        pl: 1,
        pr: 0,
        top: '-5px',
        left: '-5px',
        width: `calc(100% + 6px)`,
        height: 'calc(100% + 10px)',
        borderRadius: 'base',
        borderColor: 'address.highlighted.border',
        borderWidth: '1px',
        borderStyle: 'dashed',
        bgColor: 'address.highlighted.bg',
        zIndex: -1,
      },
      '& .entity__shield': {
        borderColor: 'address.highlighted.bg',
        bgColor: 'address.highlighted.bg',
      },
    },
  },
  '.address-entity_no-copy': {
    '&.address-entity_highlighted': {
      _before: {
        pr: 2,
        width: `calc(100% + 6px + 8px)`,
      },
    },
  },
};

export default styles;
