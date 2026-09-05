import chevronRight from 'lucide-static/icons/chevron-right.svg?raw';
import cookie from 'lucide-static/icons/cookie.svg?raw';
import copy from 'lucide-static/icons/copy.svg?raw';
import download from 'lucide-static/icons/download.svg?raw';
import filePen from 'lucide-static/icons/file-pen.svg?raw';
import folder from 'lucide-static/icons/folder.svg?raw';
import folderOpen from 'lucide-static/icons/folder-open.svg?raw';
import link from 'lucide-static/icons/link.svg?raw';
import music from 'lucide-static/icons/music.svg?raw';
import settings from 'lucide-static/icons/settings.svg?raw';
import slidersHorizontal from 'lucide-static/icons/sliders-horizontal.svg?raw';
import x from 'lucide-static/icons/x.svg?raw';

export const ICONS = {
  'chevron-right': chevronRight,
  cookie,
  copy,
  download,
  'file-pen': filePen,
  folder,
  'folder-open': folderOpen,
  link,
  music,
  settings,
  'sliders-horizontal': slidersHorizontal,
  x,
};

export type IconName = keyof typeof ICONS;
