const baseProps = {
  xmlns: "http://www.w3.org/2000/svg",
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.75,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": "true",
};

const Icon = ({ children, className = "h-5 w-5", ...rest }) => (
  <svg {...baseProps} {...rest} className={className}>
    {children}
  </svg>
);

export const HomeIcon = (props) => (
  <Icon {...props}>
    <path d="m3 11 9-7.5 9 7.5" />
    <path d="M5 10v9a1 1 0 0 0 1 1h4v-6h4v6h4a1 1 0 0 0 1-1v-9" />
  </Icon>
);

export const CameraIcon = (props) => (
  <Icon {...props}>
    <path d="M4 7h3l1.6-2.4a1 1 0 0 1 .83-.45h5.14a1 1 0 0 1 .83.45L17 7h3a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1Z" />
    <circle cx="12" cy="13" r="3.75" />
  </Icon>
);

export const VideoIcon = (props) => (
  <Icon {...props}>
    <rect x="2.5" y="6" width="14" height="12" rx="2" />
    <path d="m16.5 10 5-3v10l-5-3" />
  </Icon>
);

export const UserIcon = (props) => (
  <Icon {...props}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4 21a8 8 0 0 1 16 0" />
  </Icon>
);

export const PlusIcon = (props) => (
  <Icon {...props}>
    <path d="M12 5v14M5 12h14" />
  </Icon>
);

export const SearchIcon = (props) => (
  <Icon {...props}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m20 20-4.3-4.3" />
  </Icon>
);

export const SparkleIcon = (props) => (
  <Icon {...props}>
    <path d="M12 3v4M12 17v4M3 12h4M17 12h4M5.5 5.5l2.8 2.8M15.7 15.7l2.8 2.8M5.5 18.5l2.8-2.8M15.7 8.3l2.8-2.8" />
  </Icon>
);

export const TrendIcon = (props) => (
  <Icon {...props}>
    <path d="m3 17 6-6 4 4 8-8" />
    <path d="M14 7h7v7" />
  </Icon>
);

export const HeartIcon = (props) => (
  <Icon {...props}>
    <path d="M12 20s-7-4.5-9.3-9.4A5.4 5.4 0 0 1 12 5.5a5.4 5.4 0 0 1 9.3 5.1C19 15.5 12 20 12 20Z" />
  </Icon>
);

export const RepostIcon = (props) => (
  <Icon {...props}>
    <path d="M7 7h12v3l3-3-3-3v3" />
    <path d="M17 17H5v-3l-3 3 3 3v-3" />
  </Icon>
);

export const CommentIcon = (props) => (
  <Icon {...props}>
    <path d="M21 12a8 8 0 0 1-11.7 7.1L4 20l1-4.6A8 8 0 1 1 21 12Z" />
  </Icon>
);

export const SmileIcon = (props) => (
  <Icon {...props}>
    <circle cx="12" cy="12" r="9" />
    <path d="M8.5 14a4 4 0 0 0 7 0" />
    <path d="M9 10h.01M15 10h.01" strokeWidth="2.2" />
  </Icon>
);

export const CloseIcon = (props) => (
  <Icon {...props}>
    <path d="M6 6l12 12M18 6L6 18" />
  </Icon>
);

export const ChevronLeftIcon = (props) => (
  <Icon {...props}>
    <path d="m15 6-6 6 6 6" />
  </Icon>
);

export const ChevronRightIcon = (props) => (
  <Icon {...props}>
    <path d="m9 6 6 6-6 6" />
  </Icon>
);

export const TrashIcon = (props) => (
  <Icon {...props}>
    <path d="M4 7h16M10 11v6M14 11v6" />
    <path d="M6 7v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V7" />
    <path d="M9 7V5a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2" />
  </Icon>
);

export const ImageIcon = (props) => (
  <Icon {...props}>
    <rect x="3" y="4.5" width="18" height="15" rx="2" />
    <circle cx="9" cy="10" r="1.6" />
    <path d="m4 18 5-5 4 4 3-3 4 4" />
  </Icon>
);

export const SunIcon = (props) => (
  <Icon {...props}>
    <circle cx="12" cy="12" r="3.5" />
    <path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4" />
  </Icon>
);

export const MoonIcon = (props) => (
  <Icon {...props}>
    <path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5Z" />
  </Icon>
);

export const LogoutIcon = (props) => (
  <Icon {...props}>
    <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
    <path d="m16 17 5-5-5-5" />
    <path d="M21 12H9" />
  </Icon>
);

export const PaperPlaneIcon = (props) => (
  <Icon {...props}>
    <path d="M21.5 2.5 2 11l7 2.5L21.5 2.5Z" />
    <path d="M21.5 2.5 11.5 22l-2.5-8.5" />
  </Icon>
);

export const BellIcon = (props) => (
  <Icon {...props}>
    <path d="M6 8a6 6 0 0 1 12 0c0 6 2 7 2 7H4s2-1 2-7Z" />
    <path d="M10 19a2 2 0 0 0 4 0" />
  </Icon>
);

export const CheckIcon = (props) => (
  <Icon {...props}>
    <path d="m4.5 12.5 5 5 10-11" />
  </Icon>
);

export const QRIcon = (props) => (
  <Icon {...props}>
    <rect x="3" y="3" width="7" height="7" rx="1" />
    <rect x="5.5" y="5.5" width="2" height="2" fill="currentColor" stroke="none" />
    <rect x="14" y="3" width="7" height="7" rx="1" />
    <rect x="16.5" y="5.5" width="2" height="2" fill="currentColor" stroke="none" />
    <rect x="3" y="14" width="7" height="7" rx="1" />
    <rect x="5.5" y="16.5" width="2" height="2" fill="currentColor" stroke="none" />
    <path d="M14 14h2v2h-2zM18 14h3M14 18h3M19 17v4M21 18v3" />
  </Icon>
);

export const ChatIcon = (props) => (
  <Icon {...props}>
    <path d="M4 5h16a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H8l-4 3.5V6a1 1 0 0 1 1-1Z" />
    <circle cx="9" cy="11" r="0.6" fill="currentColor" stroke="none" />
    <circle cx="12" cy="11" r="0.6" fill="currentColor" stroke="none" />
    <circle cx="15" cy="11" r="0.6" fill="currentColor" stroke="none" />
  </Icon>
);

export const ShieldCheckIcon = (props) => (
  <Icon {...props}>
    <path d="M12 3 4.5 6v6.4c0 4 3 7.4 7.5 8.6 4.5-1.2 7.5-4.6 7.5-8.6V6L12 3Z" />
    <path d="m9 12 2 2 4-4" />
  </Icon>
);

export const PaperclipIcon = (props) => (
  <Icon {...props}>
    <path d="M20 11.5 12.5 19a5 5 0 0 1-7-7l8-8a3.5 3.5 0 0 1 5 5l-8 8a2 2 0 0 1-3-3l7-7" />
  </Icon>
);

export const PlayIcon = (props) => (
  <Icon {...props}>
    <path d="M8 5.5v13l11-6.5Z" fill="currentColor" stroke="none" />
  </Icon>
);
