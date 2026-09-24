import { recipe as accordion } from './accordion.recipe';
import { recipe as alert } from './alert.recipe';
import { recipe as badge } from './badge.recipe';
import { recipe as button } from './button.recipe';
import { recipe as checkbox } from './checkbox.recipe';
import { recipe as checkmark } from './checkmark.recipe';
import { recipe as closeButton } from './close-button.recipe';
import { recipe as code } from './code.recipe';
import { recipe as dialog } from './dialog.recipe';
import { recipe as drawer } from './drawer.recipe';
import { recipe as emptyState } from './empty-state.recipe';
import { recipe as field } from './field.recipe';
import { recipe as input } from './input.recipe';
import { recipe as link } from './link.recipe';
import { recipe as list } from './list.recipe';
import { recipe as menu } from './menu.recipe';
import { recipe as pinInput } from './pin-input.recipe';
import { recipe as popover } from './popover.recipe';
import { recipe as progressCircle } from './progress-circle.recipe';
import { recipe as progress } from './progress.recipe';
import { recipe as radioGroup } from './radio-group.recipe';
import { recipe as radiomark } from './radiomark.recipe';
import { recipe as ratingGroup } from './rating-group.recipe';
import { recipe as select } from './select.recipe';
import { recipe as separator } from './separator.recipe';
import { recipe as skeleton } from './skeleton.recipe';
import { recipe as spinner } from './spinner.recipe';
import { recipe as stat } from './stat.recipe';
import { recipe as switchRecipe } from './switch.recipe';
import { recipe as table } from './table.recipe';
import { recipe as tabs } from './tabs.recipe';
import { recipe as tag } from './tag.recipe';
import { recipe as textarea } from './textarea.recipe';
import { recipe as toast } from './toast.recipe';
import { recipe as tooltip } from './tooltip.recipe';

export const recipes = {
  badge,
  button,
  code,
  checkmark,
  closeButton,
  input,
  link,
  radiomark,
  skeleton,
  spinner,
  separator,
  textarea,
};

export const slotRecipes = {
  accordion,
  alert,
  checkbox,
  dialog,
  drawer,
  emptyState,
  field,
  list,
  menu,
  pinInput,
  popover,
  progress,
  progressCircle,
  radioGroup,
  ratingGroup,
  select,
  stat,
  'switch': switchRecipe,
  table,
  tabs,
  tag,
  toast,
  tooltip,
};
