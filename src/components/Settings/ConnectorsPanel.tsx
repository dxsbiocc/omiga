import {
  memo,
  type ReactNode,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useState,
} from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Alert,
  Box,
  Button,
  Card,
  CardContent,
  Paper,
  Chip,
  CircularProgress,
  Dialog,
  DialogContent,
  IconButton,
  MenuItem,
  Stack,
  TextField,
  Tooltip,
  Typography,
} from "@mui/material";
import { alpha, useTheme } from "@mui/material/styles";
import {
  AddRounded,
  CheckCircleRounded,
  CloseRounded,
  CloudOffRounded,
  ContentCopyRounded,
  GitHub as GitHubIcon,
  LinkRounded,
  OpenInNewRounded,
  TroubleshootRounded,
} from "@mui/icons-material";
import {
  type ConnectorConnectionStatus,
  type ConnectorConnectionTestResult,
  type ConnectorDefinitionSource,
  type ConnectorAuditEvent,
  type ConnectorAuditOutcome,
  type ConnectorToolDefinition,
  type ConnectorToolExecution,
  type ConnectorHealthSummary,
  type ConnectorInfo,
  type ConnectorLoginPollResult,
  type ConnectorLoginStartResult,
  type MailConnectorCredentialRequest,
  useConnectorStore,
} from "../../state/connectorStore";
import { extractErrorMessage } from "../../utils/errorMessage";

const connectorCardGridSx = {
  display: "grid",
  gridTemplateColumns: { xs: "1fr", lg: "repeat(2, minmax(0, 1fr))" },
  gap: 1,
};

type ConnectorStatusFilter = "all" | ConnectorConnectionStatus;
type ConnectorSourceFilter = "all" | ConnectorDefinitionSource;
type ConnectorNoticeSeverity = "success" | "info" | "warning" | "error";

type ConnectorNotice = {
  severity: ConnectorNoticeSeverity;
  message: string;
};

type ConnectorAuthFlowStatus =
  | "opening"
  | "waiting"
  | "checking"
  | "setup_required"
  | "error";

type ConnectorAuthFlow = {
  loginSessionId: string;
  provider: string;
  status: ConnectorAuthFlowStatus;
  intervalSecs: number;
  message: string;
};

type MailCredentialDraft = {
  emailAddress: string;
  authorizationCode: string;
};

function statusLabel(status: ConnectorConnectionStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "needs_auth":
      return "Needs auth";
    case "disabled":
      return "Disabled";
    case "metadata_only":
      return "Plugin reference";
    default:
      return status;
  }
}

function statusColor(
  status: ConnectorConnectionStatus,
): "success" | "warning" | "default" {
  switch (status) {
    case "connected":
      return "success";
    case "needs_auth":
      return "warning";
    default:
      return "default";
  }
}

type ConnectorIconSpec = {
  bg: string;
  fg: string;
  border?: string;
  label?: string;
};

const connectorIconifyBodies: Record<string, string> = {
  asana:
    '<path fill="currentColor" d="M18.78 12.653a5.22 5.22 0 1 0 0 10.44a5.22 5.22 0 0 0 0-10.44m-13.56 0a5.22 5.22 0 1 0 .001 10.439a5.22 5.22 0 0 0-.001-10.439m12-6.525a5.22 5.22 0 1 1-10.44 0a5.22 5.22 0 0 1 10.44 0"/>',
  azure_devops:
    '<path fill="currentColor" d="M0 8.877L2.247 5.91l8.405-3.416V.022l7.37 5.393L2.966 8.338v8.225L0 15.707zm24-4.45v14.651l-5.753 4.9l-9.303-3.057v3.056l-5.978-7.416l15.057 1.798V5.415z"/>',
  bitbucket:
    '<path fill="currentColor" d="M.778 1.213a.768.768 0 0 0-.768.892l3.263 19.81c.084.5.515.868 1.022.873H19.95a.77.77 0 0 0 .77-.646l3.27-20.03a.768.768 0 0 0-.768-.891zM14.52 15.53H9.522L8.17 8.466h7.561z"/>',
  confluence:
    '<path fill="currentColor" d="M.87 18.257c-.248.382-.53.875-.763 1.245a.764.764 0 0 0 .255 1.04l4.965 3.054a.764.764 0 0 0 1.058-.26c.199-.332.454-.763.733-1.221c1.967-3.247 3.945-2.853 7.508-1.146l4.957 2.337a.764.764 0 0 0 1.028-.382l2.364-5.346a.764.764 0 0 0-.382-1a600 600 0 0 1-4.965-2.361C10.911 10.97 5.224 11.185.87 18.257M23.131 5.743c.249-.405.531-.875.764-1.25a.764.764 0 0 0-.256-1.034L18.675.404a.764.764 0 0 0-1.058.26a66 66 0 0 1-.734 1.225c-1.966 3.246-3.945 2.85-7.508 1.146L4.437.694a.764.764 0 0 0-1.027.382L1.046 6.422a.764.764 0 0 0 .382 1c1.039.49 3.105 1.467 4.965 2.361c6.698 3.246 12.392 3.029 16.738-4.04"/>',
  discord:
    '<path fill="currentColor" d="M20.317 4.37a19.8 19.8 0 0 0-4.885-1.515a.074.074 0 0 0-.079.037c-.21.375-.444.864-.608 1.25a18.3 18.3 0 0 0-5.487 0a13 13 0 0 0-.617-1.25a.08.08 0 0 0-.079-.037A19.7 19.7 0 0 0 3.677 4.37a.1.1 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.057a.08.08 0 0 0 .031.057a19.9 19.9 0 0 0 5.993 3.03a.08.08 0 0 0 .084-.028a14 14 0 0 0 1.226-1.994a.076.076 0 0 0-.041-.106a13 13 0 0 1-1.872-.892a.077.077 0 0 1-.008-.128a10 10 0 0 0 .372-.292a.07.07 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.07.07 0 0 1 .078.01q.181.149.373.292a.077.077 0 0 1-.006.127a12.3 12.3 0 0 1-1.873.892a.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.08.08 0 0 0 .084.028a19.8 19.8 0 0 0 6.002-3.03a.08.08 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.06.06 0 0 0-.031-.03M8.02 15.33c-1.182 0-2.157-1.085-2.157-2.419c0-1.333.956-2.419 2.157-2.419c1.21 0 2.176 1.096 2.157 2.42c0 1.333-.956 2.418-2.157 2.418m7.975 0c-1.183 0-2.157-1.085-2.157-2.419c0-1.333.955-2.419 2.157-2.419c1.21 0 2.176 1.096 2.157 2.42c0 1.333-.946 2.418-2.157 2.418"/>',
  dropbox:
    '<path fill="currentColor" d="M6 1.807L0 5.629l6 3.822l6.001-3.822zm12 0l-6 3.822l6 3.822l6-3.822zM0 13.274l6 3.822l6.001-3.822L6 9.452zm18-3.822l-6 3.822l6 3.822l6-3.822zM6 18.371l6.001 3.822l6-3.822l-6-3.822z"/>',
  figma:
    '<path fill="currentColor" d="M15.852 8.981h-4.588V0h4.588c2.476 0 4.49 2.014 4.49 4.49s-2.014 4.491-4.49 4.491M12.735 7.51h3.117c1.665 0 3.019-1.355 3.019-3.019s-1.355-3.019-3.019-3.019h-3.117zm0 1.471H8.148c-2.476 0-4.49-2.014-4.49-4.49S5.672 0 8.148 0h4.588v8.981zm-4.587-7.51c-1.665 0-3.019 1.355-3.019 3.019s1.354 3.02 3.019 3.02h3.117V1.471zm4.587 15.019H8.148c-2.476 0-4.49-2.014-4.49-4.49s2.014-4.49 4.49-4.49h4.588v8.98zM8.148 8.981c-1.665 0-3.019 1.355-3.019 3.019s1.355 3.019 3.019 3.019h3.117V8.981zM8.172 24c-2.489 0-4.515-2.014-4.515-4.49s2.014-4.49 4.49-4.49h4.588v4.441c0 2.503-2.047 4.539-4.563 4.539m-.024-7.51a3.023 3.023 0 0 0-3.019 3.019c0 1.665 1.365 3.019 3.044 3.019c1.705 0 3.093-1.376 3.093-3.068v-2.97zm7.704 0h-.098c-2.476 0-4.49-2.014-4.49-4.49s2.014-4.49 4.49-4.49h.098c2.476 0 4.49 2.014 4.49 4.49s-2.014 4.49-4.49 4.49m-.097-7.509c-1.665 0-3.019 1.355-3.019 3.019s1.355 3.019 3.019 3.019h.098c1.665 0 3.019-1.355 3.019-3.019s-1.355-3.019-3.019-3.019z"/>',
  gitlab:
    '<path fill="currentColor" d="m23.6 9.593l-.033-.086L20.3.98a.85.85 0 0 0-.336-.405a.875.875 0 0 0-1 .054a.9.9 0 0 0-.29.44L16.47 7.818H7.537L5.333 1.07a.86.86 0 0 0-.29-.441a.875.875 0 0 0-1-.054a.86.86 0 0 0-.336.405L.433 9.502l-.032.086a6.066 6.066 0 0 0 2.012 7.01l.01.009l.03.021l4.977 3.727l2.462 1.863l1.5 1.132a1.01 1.01 0 0 0 1.22 0l1.499-1.132l2.461-1.863l5.006-3.75l.013-.01a6.07 6.07 0 0 0 2.01-7.002"/>',
  google_calendar:
    '<path fill="currentColor" d="M18.316 5.684H24v12.632h-5.684zM5.684 24h12.632v-5.684H5.684zM18.316 5.684V0H1.895A1.894 1.894 0 0 0 0 1.895v16.421h5.684V5.684zm-7.207 6.25v-.065q.407-.216.687-.617c.28-.401.279-.595.279-.982q0-.568-.3-1.025a2.05 2.05 0 0 0-.832-.714a2.7 2.7 0 0 0-1.197-.257q-.9 0-1.481.467q-.579.467-.793 1.078l1.085.452q.13-.374.413-.633q.284-.258.767-.257q.495 0 .816.264a.86.86 0 0 1 .322.703q0 .495-.36.778t-.886.284h-.567v1.085h.633q.611 0 1.02.327q.407.327.407.843q0 .505-.387.832c-.387.327-.565.327-.924.327q-.527 0-.897-.311q-.372-.312-.521-.881l-1.096.452q.268.923.977 1.401q.707.479 1.538.477a2.84 2.84 0 0 0 1.293-.291q.574-.29.902-.794q.327-.505.327-1.149q0-.643-.344-1.105a2.07 2.07 0 0 0-.881-.689m2.093-1.931l.602.913L15 10.045v5.744h1.187V8.446h-.827zM22.105 0h-3.289v5.184H24V1.895A1.894 1.894 0 0 0 22.105 0m-3.289 23.5l4.684-4.684h-4.684zM0 22.105C0 23.152.848 24 1.895 24h3.289v-5.184H0z"/>',
  google_drive:
    '<path fill="currentColor" d="M12.01 1.485c-2.082 0-3.754.02-3.743.047c.01.02 1.708 3.001 3.774 6.62l3.76 6.574h3.76c2.081 0 3.753-.02 3.742-.047c-.005-.02-1.708-3.001-3.775-6.62l-3.76-6.574zm-4.76 1.73a789.828 789.861 0 0 0-3.63 6.319L0 15.868l1.89 3.298l1.885 3.297l3.62-6.335l3.618-6.33l-1.88-3.287C8.1 4.704 7.255 3.22 7.25 3.214zm2.259 12.653l-.203.348c-.114.198-.96 1.672-1.88 3.287a423.93 423.948 0 0 1-1.698 2.97c-.01.026 3.24.042 7.222.042h7.244l1.796-3.157c.992-1.734 1.85-3.23 1.906-3.323l.104-.167h-7.249z"/>',
  google_sheets:
    '<path fill="currentColor" d="M11.318 12.545H7.91v-1.909h3.41v1.91zM14.728 0v6h6zm1.363 10.636h-3.41v1.91h3.41zm0 3.273h-3.41v1.91h3.41zM20.727 6.5v15.864c0 .904-.732 1.636-1.636 1.636H4.909a1.636 1.636 0 0 1-1.636-1.636V1.636C3.273.732 4.005 0 4.909 0h9.318v6.5zm-3.273 2.773H6.545v7.909h10.91v-7.91zm-6.136 4.636H7.91v1.91h3.41v-1.91z"/>',
  jira:
    '<path fill="currentColor" d="M11.571 11.513H0a5.22 5.22 0 0 0 5.232 5.215h2.13v2.057A5.215 5.215 0 0 0 12.575 24V12.518a1.005 1.005 0 0 0-1.005-1.005zm5.723-5.756H5.736a5.215 5.215 0 0 0 5.215 5.214h2.129v2.058a5.22 5.22 0 0 0 5.215 5.214V6.758a1 1 0 0 0-1.001-1.001M23.013 0H11.455a5.215 5.215 0 0 0 5.215 5.215h2.129v2.057A5.215 5.215 0 0 0 24 12.483V1.005A1 1 0 0 0 23.013 0"/>',
  linear:
    '<path fill="currentColor" d="M2.886 4.18A11.98 11.98 0 0 1 11.99 0C18.624 0 24 5.376 24 12.009c0 3.64-1.62 6.903-4.18 9.105L2.887 4.18ZM1.817 5.626l16.556 16.556q-.787.496-1.65.866L.951 7.277q.371-.863.866-1.65ZM.322 9.163l14.515 14.515q-1.066.26-2.195.322L0 11.358a12 12 0 0 1 .322-2.195m-.17 4.862l9.823 9.824a12.02 12.02 0 0 1-9.824-9.824Z"/>',
  microsoft_teams:
    '<path fill="currentColor" d="M20.625 8.127q-.55 0-1.025-.205t-.832-.563t-.563-.832T18 5.502q0-.54.205-1.02t.563-.837q.357-.358.832-.563q.474-.205 1.025-.205q.54 0 1.02.205t.837.563q.358.357.563.837t.205 1.02q0 .55-.205 1.025t-.563.832q-.357.358-.837.563t-1.02.205m0-3.75q-.469 0-.797.328t-.328.797t.328.797t.797.328t.797-.328t.328-.797t-.328-.797t-.797-.328M24 10.002v5.578q0 .774-.293 1.46t-.803 1.194q-.51.51-1.195.803q-.686.293-1.459.293q-.445 0-.908-.105q-.463-.106-.85-.329q-.293.95-.855 1.729t-1.319 1.336t-1.67.861t-1.898.305q-1.148 0-2.162-.398q-1.014-.399-1.805-1.102t-1.312-1.664t-.674-2.086h-5.8q-.411 0-.704-.293T0 16.881V6.873q0-.41.293-.703t.703-.293h8.59q-.34-.715-.34-1.5q0-.727.275-1.365q.276-.639.75-1.114q.475-.474 1.114-.75q.638-.275 1.365-.275t1.365.275t1.114.75q.474.475.75 1.114q.275.638.275 1.365t-.275 1.365q-.276.639-.75 1.113q-.475.475-1.114.75q-.638.276-1.365.276q-.188 0-.375-.024q-.188-.023-.375-.058v1.078h10.875q.469 0 .797.328t.328.797M12.75 2.373q-.41 0-.78.158q-.368.158-.638.434q-.27.275-.428.639q-.158.363-.158.773t.158.78q.159.368.428.638q.27.27.639.428t.779.158t.773-.158q.364-.159.64-.428q.274-.27.433-.639t.158-.779t-.158-.773q-.159-.364-.434-.64q-.275-.275-.639-.433q-.363-.158-.773-.158M6.937 9.814h2.25V7.94H2.814v1.875h2.25v6h1.875zm10.313 7.313v-6.75H12v6.504q0 .41-.293.703t-.703.293H8.309q.152.809.556 1.5q.405.691.985 1.19q.58.497 1.318.779q.738.281 1.582.281q.926 0 1.746-.352q.82-.351 1.436-.966q.615-.616.966-1.43q.352-.815.352-1.752m5.25-1.547v-5.203h-3.75v6.855q.305.305.691.452q.387.146.809.146q.469 0 .879-.176q.41-.175.715-.48q.304-.305.48-.715t.176-.879"/>',
  notion:
    '<path fill="currentColor" d="M4.459 4.208c.746.606 1.026.56 2.428.466l13.215-.793c.28 0 .047-.28-.046-.326L17.86 1.968c-.42-.326-.981-.7-2.055-.607L3.01 2.295c-.466.046-.56.28-.374.466zm.793 3.08v13.904c0 .747.373 1.027 1.214.98l14.523-.84c.841-.046.935-.56.935-1.167V6.354c0-.606-.233-.933-.748-.887l-15.177.887c-.56.047-.747.327-.747.933zm14.337.745c.093.42 0 .84-.42.888l-.7.14v10.264c-.608.327-1.168.514-1.635.514c-.748 0-.935-.234-1.495-.933l-4.577-7.186v6.952L12.21 19s0 .84-1.168.84l-3.222.186c-.093-.186 0-.653.327-.746l.84-.233V9.854L7.822 9.76c-.094-.42.14-1.026.793-1.073l3.456-.233l4.764 7.279v-6.44l-1.215-.139c-.093-.514.28-.887.747-.933zM1.936 1.035l13.31-.98c1.634-.14 2.055-.047 3.082.7l4.249 2.986c.7.513.934.653.934 1.213v16.378c0 1.026-.373 1.634-1.68 1.726l-15.458.934c-.98.047-1.448-.093-1.962-.747l-3.129-4.06c-.56-.747-.793-1.306-.793-1.96V2.667c0-.839.374-1.54 1.447-1.632"/>',
  outlook:
    '<path fill="currentColor" d="M7.88 12.04q0 .45-.11.87q-.1.41-.33.74q-.22.33-.58.52q-.37.2-.87.2t-.85-.2q-.35-.21-.57-.55q-.22-.33-.33-.75q-.1-.42-.1-.86t.1-.87t.34-.76q.22-.34.59-.54q.36-.2.87-.2t.86.2q.35.21.57.55t.31.77q.1.43.1.88M24 12v9.38q0 .46-.33.8q-.33.32-.8.32H7.13q-.46 0-.8-.33q-.32-.33-.32-.8V18H1q-.41 0-.7-.3q-.3-.29-.3-.7V7q0-.41.3-.7Q.58 6 1 6h6.5V2.55q0-.44.3-.75q.3-.3.75-.3h12.9q.44 0 .75.3q.3.3.3.75v8.3l1.24.72h.01q.1.07.18.18q.07.12.07.25m-6-8.25v3h3v-3zm0 4.5v3h3v-3zm0 4.5v1.83l3.05-1.83zm-5.25-9v3h3.75v-3zm0 4.5v3h3.75v-3zm0 4.5v2.03l2.41 1.5l1.34-.8v-2.73zM9 3.75V6h2l.13.01l.12.04v-2.3zM5.98 15.98q.9 0 1.6-.3q.7-.32 1.19-.86q.48-.55.73-1.28q.25-.74.25-1.61q0-.83-.25-1.55q-.24-.71-.71-1.24t-1.15-.83t-1.55-.3q-.92 0-1.64.3q-.71.3-1.2.85q-.5.54-.75 1.3q-.25.74-.25 1.63q0 .85.26 1.56q.26.72.74 1.23q.48.52 1.17.81q.69.3 1.56.3zM7.5 21h12.39L12 16.08V17q0 .41-.3.7q-.29.3-.7.3H7.5zm15-.13v-7.24l-5.9 3.54Z"/>',
  qq_mail:
    '<path fill="currentColor" d="M14.57 10.18c-.06-1.4-.97-2.52-2.45-2.52s-2.39 1.11-2.45 2.52c-.04.08-.06.17-.06.26v.04c-.1.16-.16.35-.16.56v.1c-.24.09-.55.49-.78 1.02c-.29.69-.34 1.34-.1 1.46c.16.09.41-.11.65-.46c.09.38.33.73.66 1.01c-.35.13-.57.34-.57.58c0 .39.61.71 1.37.71c.68 0 1.25-.26 1.35-.59h.16c.1.34.67.59 1.36.59c.76 0 1.37-.32 1.37-.71c0-.24-.23-.45-.57-.58c.33-.28.56-.63.66-1.01c.24.35.49.54.65.46c.23-.12.19-.78-.1-1.46c-.23-.54-.54-.94-.78-1.02v-.1c0-.21-.06-.4-.16-.56v-.04c0-.1-.02-.19-.06-.26Zm.01-7.94c-1.02-.29-1.88.36-2.17 1.37c-.28 1.02.11 2.01 1.14 2.3c1.02.29 7.5 2.54 7.73 9.88c.13-.33.24-.66.34-1c1.52-5.41-1.63-11.03-7.04-12.55m2.19 14.03c-.77.73-6.03 5.14-12.45 1.57q.315.42.69.81c3.86 4.08 10.3 4.26 14.39.4c.77-.73.66-1.8-.06-2.57s-1.79-.94-2.56-.21Zm-10.8-2.48c-.23-1.04-1.32-7.81 5.04-11.49c-.35.04-.7.1-1.04.18C4.48 3.7 1.02 9.14 2.25 14.62c.23 1.04 1.21 1.49 2.24 1.26s1.72-1.05 1.49-2.09Z"/>',
  sentry:
    '<path fill="currentColor" d="M13.91 2.505c-.873-1.448-2.972-1.448-3.844 0L6.904 7.92a15.48 15.48 0 0 1 8.53 12.811h-2.221A13.3 13.3 0 0 0 5.784 9.814l-2.926 5.06a7.65 7.65 0 0 1 4.435 5.848H2.194a.365.365 0 0 1-.298-.534l1.413-2.402a5.2 5.2 0 0 0-1.614-.913L.296 19.275a2.18 2.18 0 0 0 .812 2.999a2.24 2.24 0 0 0 1.086.288h6.983a9.32 9.32 0 0 0-3.845-8.318l1.11-1.922a11.47 11.47 0 0 1 4.95 10.24h5.915a17.24 17.24 0 0 0-7.885-15.28l2.244-3.845a.37.37 0 0 1 .504-.13c.255.14 9.75 16.708 9.928 16.9a.365.365 0 0 1-.327.543h-2.287q.043.918 0 1.831h2.297a2.206 2.206 0 0 0 1.922-3.31z"/>',
  trello:
    '<path fill="currentColor" d="M21.147 0H2.853A2.86 2.86 0 0 0 0 2.853v18.294A2.86 2.86 0 0 0 2.853 24h18.294A2.86 2.86 0 0 0 24 21.147V2.853A2.86 2.86 0 0 0 21.147 0M10.34 17.287a.953.953 0 0 1-.953.953h-4a.954.954 0 0 1-.954-.953V5.38a.953.953 0 0 1 .954-.953h4a.954.954 0 0 1 .953.953zm9.233-5.467a.944.944 0 0 1-.953.947h-4a.947.947 0 0 1-.953-.947V5.38a.953.953 0 0 1 .953-.953h4a.954.954 0 0 1 .953.953z"/>',
};

const connectorIconSpecs: Record<string, ConnectorIconSpec> = {
  asana: { bg: "#FC636B", fg: "#FFFFFF", label: "A" },
  azure_devops: { bg: "#0078D4", fg: "#FFFFFF", label: "AZ" },
  bitbucket: { bg: "#0052CC", fg: "#FFFFFF", label: "BB" },
  confluence: { bg: "#172B4D", fg: "#FFFFFF", label: "CF" },
  discord: { bg: "#5865F2", fg: "#FFFFFF", label: "D" },
  dropbox: { bg: "#0061FF", fg: "#FFFFFF", label: "DB" },
  figma: { bg: "#1F2937", fg: "#FFFFFF", label: "F" },
  gitlab: { bg: "#FC6D26", fg: "#FFFFFF", label: "GL" },
  github: { bg: "#0D1117", fg: "#FFFFFF" },
  gmail: { bg: "#FFFFFF", fg: "#EA4335", border: "#DADCE0" },
  google_calendar: { bg: "#FFFFFF", fg: "#1A73E8", border: "#DADCE0", label: "31" },
  google_drive: { bg: "#FFFFFF", fg: "#188038", border: "#DADCE0" },
  google_sheets: { bg: "#0F9D58", fg: "#FFFFFF", label: "S" },
  jira: { bg: "#0052CC", fg: "#FFFFFF", label: "J" },
  linear: { bg: "#5E6AD2", fg: "#FFFFFF", label: "L" },
  microsoft_teams: { bg: "#6264A7", fg: "#FFFFFF", label: "T" },
  netease_mail: { bg: "#D81E06", fg: "#FFFFFF", label: "163" },
  notion: { bg: "#FFFFFF", fg: "#111827", border: "#DADCE0", label: "N" },
  outlook: { bg: "#0078D4", fg: "#FFFFFF", label: "O" },
  qq_mail: { bg: "#12B7F5", fg: "#FFFFFF", label: "QQ" },
  sentry: { bg: "#362D59", fg: "#FFFFFF", label: "S" },
  slack: { bg: "#FFFFFF", fg: "#4A154B", border: "#DADCE0" },
  trello: { bg: "#0079BF", fg: "#FFFFFF", label: "T" },
};

const categoryLabels: Record<string, string> = {
  code: "Code",
  communication: "Communication",
  design: "Design",
  email: "Email",
  knowledge: "Knowledge",
  observability: "Ops",
  other: "Other",
  plugin: "Plugin",
  productivity: "Productivity",
  project_management: "Project",
  storage: "Storage",
};

function categoryLabel(value: string): string {
  return categoryLabels[value] ?? value.replace(/[-_]+/g, "");
}

function authHint(connector: ConnectorInfo): string {
  const auth = connector.definition.authType;
  if (connector.definition.id === "github") {
    return "Use GitHub login or GitHub CLI (gh auth login); env tokens are advanced fallbacks";
  }
  if (connector.definition.id === "notion") {
    return "Use Notion login/authorization; env tokens are advanced fallbacks";
  }
  if (connector.definition.id === "slack") {
    return "Use Slack login/authorization; env bot tokens are advanced fallbacks";
  }
  if (connector.definition.id === "gmail") {
    return "输入 Gmail 地址和 Google 应用专用密码；Omiga 保存到系统安全存储并完成本机校验";
  }
  if (connector.definition.id === "qq_mail") {
    return "输入 QQ 邮箱账号和授权码；Omiga 保存到系统安全存储并完成本机校验";
  }
  if (connector.definition.id === "netease_mail") {
    return "输入网易邮箱账号和授权码；Omiga 保存到系统安全存储并完成本机校验";
  }
  if (auth === "none") return "No authentication required";
  if (auth === "externalMcp")
    return "Declared by plugin; add a matching MCP/tool integration";
  if (connector.definition.envVars.length > 0) {
    return "Use the official connection page or software login; advanced credentials stay outside Omiga config";
  }
  if (auth === "oauth") return "Browser/software authorization";
  if (auth === "apiKey") return "Official authorization or external secret manager";
  return "External authorization required";
}

function testKindLabel(result: ConnectorConnectionTestResult): string {
  return result.checkKind === "native_api"
    ? "Live API check"
    : "Local state check";
}

function formatCheckedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function testDetailChips(result: ConnectorConnectionTestResult): Array<{
  key: string;
  label: string;
  color?: "default" | "warning" | "error";
}> {
  const chips: Array<{
    key: string;
    label: string;
    color?: "default" | "warning" | "error";
  }> = [];
  if (result.httpStatus) {
    chips.push({
      key: "http",
      label: `HTTP ${result.httpStatus}`,
      color: result.httpStatus >= 400 ? "error" : "default",
    });
  }
  if (result.retryable) {
    chips.push({ key: "retryable", label: "Retryable", color: "warning" });
  }
  if (result.errorCode) {
    chips.push({ key: "error", label: result.errorCode, color: "default" });
  }
  return chips;
}

function auditOutcomeLabel(outcome: ConnectorAuditOutcome): string {
  switch (outcome) {
    case "ok":
      return "成功";
    case "blocked":
      return "已拦截";
    case "error":
      return "失败";
    default:
      return outcome;
  }
}

function auditOutcomeColor(
  outcome: ConnectorAuditOutcome,
): "success" | "warning" | "error" | "default" {
  switch (outcome) {
    case "ok":
      return "success";
    case "blocked":
      return "warning";
    case "error":
      return "error";
    default:
      return "default";
  }
}

function connectionHealthColor(
  health?: ConnectorHealthSummary,
): "success" | "warning" | "error" | "default" {
  if (!health || health.totalChecks === 0) return "default";
  if (health.failedChecks === 0) return "success";
  if (health.okChecks > 0) return "warning";
  return "error";
}

function connectionHealthLabel(health?: ConnectorHealthSummary): string {
  if (!health || health.totalChecks === 0) return "No checks yet";
  return `Health ${health.okChecks}/${health.totalChecks} OK`;
}

function connectionHealthDetail(
  health?: ConnectorHealthSummary,
): string | null {
  if (!health || health.totalChecks === 0) return null;
  const parts = [
    `${health.failedChecks} failed`,
    health.retryableFailures > 0
      ? `${health.retryableFailures} retryable`
      : null,
    health.lastFailureAt
      ? `last failure ${formatCheckedAt(health.lastFailureAt)}`
      : null,
    health.lastErrorCode ?? null,
    health.lastHttpStatus ? `HTTP ${health.lastHttpStatus}` : null,
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : "All recent checks passed";
}

function sourceLabel(connector: ConnectorInfo): string {
  return sourceValueLabel(connector.source);
}

function sourceValueLabel(source: ConnectorDefinitionSource): string {
  switch (source) {
    case "custom":
      return "Custom";
    case "plugin":
      return "Plugin";
    default:
      return "Built-in";
  }
}

function connectorMatchesSearch(
  connector: ConnectorInfo,
  query: string,
): boolean {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) return true;
  const haystack = [
    connector.definition.id,
    connector.definition.name,
    connector.definition.description,
    connector.definition.category,
    connector.source,
    ...connector.definition.envVars,
    ...connector.definition.tools.map(
      (tool) => `${tool.name} ${tool.description}`,
    ),
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(normalizedQuery);
}

type ConnectorCardProps = {
  connector: ConnectorInfo;
  busy: boolean;
  testResult?: ConnectorConnectionTestResult;
  onQuickAdd: (connector: ConnectorInfo) => void;
  onOpenDetails: (connector: ConnectorInfo) => void;
};

const ConnectorCard = memo(function ConnectorCard({
  connector,
  busy,
  testResult,
  onQuickAdd,
  onOpenDetails,
}: ConnectorCardProps) {
  const theme = useTheme();
  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const isReady = connector.accessible;
  const needsAttention =
    isProductIntegrated &&
    connector.enabled &&
    (!connector.accessible || testResult?.ok === false);
  const subtitle = connector.definition.description;

  const openDetails = () => onOpenDetails(connector);

  return (
    <Paper
      variant="outlined"
      role="button"
      tabIndex={0}
      aria-label={`Open ${connector.definition.name} connector details`}
      aria-disabled={!isProductIntegrated}
      onClick={openDetails}
      onKeyDown={(event) => {
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        openDetails();
      }}
      sx={{
        px: 1.25,
        py: 1.15,
        minHeight: 72,
        borderRadius: 2.5,
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 1.25,
        bgcolor: !isProductIntegrated
          ? alpha(
              theme.palette.text.disabled,
              theme.palette.mode === "dark" ? 0.1 : 0.06,
            )
          : "background.paper",
        borderColor: needsAttention
          ? alpha(theme.palette.warning.main, 0.36)
          : !isProductIntegrated
            ? alpha(theme.palette.text.disabled, 0.22)
            : "transparent",
        boxShadow: "none",
        opacity: !isProductIntegrated ? 0.68 : 1,
        transition:
          "background-color 160ms ease, box-shadow 160ms ease, transform 160ms ease",
        "@media (prefers-reduced-motion: reduce)": {
          transition: "none",
        },
        "&:hover": {
          bgcolor: !isProductIntegrated
            ? alpha(
                theme.palette.text.disabled,
                theme.palette.mode === "dark" ? 0.12 : 0.08,
              )
            : "action.hover",
          boxShadow: !isProductIntegrated
            ? "none"
            : `0 8px 22px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.24 : 0.07)}`,
          transform: !isProductIntegrated ? "none" : "translateY(-1px)",
        },
        "&:focus-visible": {
          outline: `2px solid ${alpha(theme.palette.primary.main, 0.7)}`,
          outlineOffset: 2,
        },
      }}
    >
      <ConnectorIcon
        connector={connector}
        size={38}
        disabled={!isProductIntegrated}
      />

      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Stack
          direction="row"
          spacing={0.75}
          alignItems="center"
          sx={{ minWidth: 0 }}
        >
          <Typography
            variant="subtitle2"
            fontWeight={800}
            noWrap
            title={connector.definition.name}
          >
            {connector.definition.name}
          </Typography>
          {connector.source !== "built_in" && (
            <Chip
              size="small"
              variant="outlined"
              label={sourceLabel(connector)}
              sx={{ height: 20, flexShrink: 0 }}
            />
          )}
          {!isProductIntegrated && (
            <Chip
              size="small"
              variant="outlined"
              label="未接入"
              sx={{
                height: 20,
                flexShrink: 0,
                color: "text.disabled",
                borderColor: "divider",
                bgcolor: "action.disabledBackground",
              }}
            />
          )}
        </Stack>
        <Typography
          variant="body2"
          color="text.secondary"
          noWrap
          title={subtitle}
          sx={{ mt: 0.15 }}
        >
          {subtitle}
        </Typography>
      </Box>

      {!isProductIntegrated ? (
        <Box
          aria-label={`${connector.definition.name} is not integrated yet`}
          title="未接入真实连接方式"
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: "text.disabled",
            bgcolor: "action.disabledBackground",
          }}
        >
          <CloudOffRounded fontSize="small" />
        </Box>
      ) : isReady ? (
        <Box
          aria-label={`${connector.definition.name} is ${statusLabel(connector.status)}`}
          title={statusLabel(connector.status)}
          sx={{
            width: 32,
            height: 32,
            borderRadius: "50%",
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            color: isReady
              ? "success.main"
              : needsAttention
                ? "warning.main"
                : "text.disabled",
          }}
        >
          <CheckCircleRounded fontSize="small" />
        </Box>
      ) : (
        <IconButton
          aria-label={`Add ${connector.definition.name}`}
          size="small"
          disabled={busy || !isProductIntegrated}
          onClick={(event) => {
            event.stopPropagation();
            onQuickAdd(connector);
          }}
          onKeyDown={(event) => event.stopPropagation()}
          sx={{
            width: 34,
            height: 34,
            flexShrink: 0,
            overflow: "hidden",
            isolation: "isolate",
            bgcolor: alpha(
              theme.palette.text.primary,
              theme.palette.mode === "dark" ? 0.12 : 0.06,
            ),
            transition:
              "transform 180ms ease, background-color 180ms ease, box-shadow 180ms ease",
            "@media (prefers-reduced-motion: reduce)": {
              transition: "none",
            },
            "&::before": {
              content: '""',
              position: "absolute",
              inset: 5,
              borderRadius: "50%",
              bgcolor: alpha(theme.palette.primary.main, 0.16),
              transform: "scale(0)",
              opacity: 0,
              transition: "transform 220ms ease, opacity 180ms ease",
            },
            "& .MuiSvgIcon-root": {
              position: "relative",
              zIndex: 1,
              transition: "transform 220ms ease",
            },
            "&:hover": {
              bgcolor: alpha(
                theme.palette.primary.main,
                theme.palette.mode === "dark" ? 0.22 : 0.1,
              ),
              boxShadow: `0 10px 24px ${alpha(theme.palette.primary.main, 0.26)}`,
              transform: "translateY(-2px) scale(1.06)",
              "&::before": {
                transform: "scale(2.5)",
                opacity: 1,
              },
              "& .MuiSvgIcon-root": {
                transform: "rotate(90deg) scale(1.12)",
              },
            },
            "&:active": {
              transform: "translateY(0) scale(0.96)",
            },
            "&.Mui-disabled": {
              boxShadow: "none",
              transform: "none",
            },
          }}
        >
          {busy ? <CircularProgress size={16} /> : <AddRounded fontSize="small" />}
        </IconButton>
      )}
    </Paper>
  );
});

function connectorInitials(connector: ConnectorInfo): string {
  return (
    connector.definition.name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((part) => part[0]?.toUpperCase() ?? "")
      .join("") || connector.definition.name.slice(0, 2).toUpperCase()
  );
}

function connectorIconLabel(connector: ConnectorInfo): string {
  return connectorIconSpecs[connector.definition.id]?.label ?? connectorInitials(connector);
}

function ConnectorBrandGlyph({
  connector,
  size,
}: {
  connector: ConnectorInfo;
  size: number;
}): ReactNode {
  const id = connector.definition.id;
  const glyphSize = Math.round(size * 0.62);
  const iconifyBody = connectorIconifyBodies[id];
  const label = connectorIconLabel(connector);
  const textSize = label.length > 2 ? size * 0.24 : size * 0.34;

  if (iconifyBody) {
    return (
      <Box
        component="svg"
        viewBox="0 0 24 24"
        aria-hidden="true"
        sx={{ width: glyphSize, height: glyphSize, display: "block" }}
        dangerouslySetInnerHTML={{ __html: iconifyBody }}
      />
    );
  }

  if (id === "github") {
    return <GitHubIcon sx={{ fontSize: glyphSize }} />;
  }

  if (id === "gmail") {
    return (
      <Box
        component="svg"
        viewBox="0 0 24 18"
        aria-hidden="true"
        sx={{ width: glyphSize, height: Math.round(glyphSize * 0.75), display: "block" }}
      >
        <path fill="#EA4335" d="M2 2.3 12 10l10-7.7V16a2 2 0 0 1-2 2h-2V7.8L12 12.4 6 7.8V18H4a2 2 0 0 1-2-2Z" />
        <path fill="#FBBC04" d="M0 4.2A2 2 0 0 1 3.2 2.6L6 4.8V18H2a2 2 0 0 1-2-2Z" />
        <path fill="#34A853" d="M18 4.8 20.8 2.6A2 2 0 0 1 24 4.2V16a2 2 0 0 1-2 2h-4Z" />
        <path fill="#4285F4" d="M6 4.8 12 9.4l6-4.6V8l-6 4.6L6 8Z" />
      </Box>
    );
  }

  if (id === "slack") {
    return (
      <Box
        component="svg"
        viewBox="0 0 24 24"
        aria-hidden="true"
        sx={{ width: glyphSize, height: glyphSize, display: "block" }}
      >
        <rect x="9.8" y="2" width="4" height="9" rx="2" fill="#36C5F0" />
        <rect x="2" y="10.2" width="9" height="4" rx="2" fill="#2EB67D" />
        <rect x="10.2" y="13" width="4" height="9" rx="2" fill="#ECB22E" />
        <rect x="13" y="9.8" width="9" height="4" rx="2" fill="#E01E5A" />
      </Box>
    );
  }

  return (
    <Box
      component="span"
      sx={{
        color: "inherit",
        fontSize: textSize,
        fontWeight: 950,
        letterSpacing: label.length > 2 ? -0.8 : -0.35,
        lineHeight: 1,
      }}
    >
      {label}
    </Box>
  );
}

function ConnectorIcon({
  connector,
  size = 38,
  disabled = false,
  rounded = 2,
}: {
  connector: ConnectorInfo;
  size?: number;
  disabled?: boolean;
  rounded?: number | string;
}) {
  const theme = useTheme();
  const spec = connectorIconSpecs[connector.definition.id] ?? {
    bg: theme.palette.background.paper,
    fg: theme.palette.text.primary,
    border: theme.palette.divider,
  };
  const borderColor = spec.border ?? alpha(spec.fg, theme.palette.mode === "dark" ? 0.32 : 0.22);

  return (
    <Box
      aria-hidden="true"
      sx={{
        width: size,
        height: size,
        borderRadius: rounded,
        display: "inline-grid",
        placeItems: "center",
        flexShrink: 0,
        overflow: "hidden",
        bgcolor: disabled
          ? alpha(theme.palette.text.disabled, theme.palette.mode === "dark" ? 0.16 : 0.1)
          : spec.bg,
        color: disabled ? "text.disabled" : spec.fg,
        border: `1px solid ${disabled ? alpha(theme.palette.text.disabled, 0.25) : borderColor}`,
        boxShadow: disabled
          ? "none"
          : `0 8px 18px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.22 : 0.08)}`,
        filter: disabled ? "grayscale(0.85)" : "none",
        opacity: disabled ? 0.72 : 1,
      }}
    >
      <ConnectorBrandGlyph connector={connector} size={size} />
    </Box>
  );
}


function connectorDeveloperLabel(connector: ConnectorInfo): string {
  if (connector.source === "custom") return "由你添加";
  if (connector.source === "plugin") return "由插件提供";
  return "由 Omiga 开发";
}

function connectorCapabilitiesLabel(connector: ConnectorInfo): string {
  const tools = connector.definition.tools;
  if (tools.length === 0) return "Metadata only";
  const readOnly = tools.every((tool) => tool.readOnly);
  const hasWrites = tools.some((tool) => !tool.readOnly);
  if (readOnly) return "Read";
  if (hasWrites && tools.some((tool) => tool.readOnly)) return "Read, Write";
  return "Write";
}

function nativeToolCount(connector: ConnectorInfo): number {
  return connector.definition.tools.filter((tool) => tool.execution === "native")
    .length;
}

function connectorSupportsCredentialValidation(connector: ConnectorInfo): boolean {
  return (
    connector.definition.id === "gmail" ||
    connector.definition.id === "qq_mail" ||
    connector.definition.id === "netease_mail"
  );
}

function connectorHasProductConnectionFlow(connector: ConnectorInfo): boolean {
  return (
    connectorSupportsLogin(connector) ||
    connectorSupportsCredentialValidation(connector)
  );
}

export function connectorIsProductIntegrated(connector: ConnectorInfo): boolean {
  return connectorHasProductConnectionFlow(connector) && connector.definition.tools.length > 0;
}

function connectorRuntimeLabel(connector: ConnectorInfo): string {
  if (!connectorIsProductIntegrated(connector)) {
    return "暂未接入";
  }
  const nativeCount = nativeToolCount(connector);
  if (nativeCount > 0) {
    return `${nativeCount} 个原生可执行工具`;
  }
  if (connectorSupportsLogin(connector)) {
    return "Omiga OAuth 登录";
  }
  if (connectorSupportsCredentialValidation(connector)) {
    return "邮箱授权码连接";
  }
  if (connector.definition.tools.length > 0) {
    return "声明能力，等待 MCP/插件/native 执行器";
  }
  return "仅元数据";
}

function toolExecutionLabel(execution?: ConnectorToolExecution): string {
  switch (execution) {
    case "native":
      return "原生可用";
    case "external_mcp":
      return "外部 MCP";
    default:
      return "声明能力";
  }
}

function toolExecutionColor(
  execution?: ConnectorToolExecution,
): "success" | "info" | "default" {
  switch (execution) {
    case "native":
      return "success";
    case "external_mcp":
      return "info";
    default:
      return "default";
  }
}

function toolAccessLabel(tool: ConnectorToolDefinition): string {
  if (tool.readOnly) return "Read";
  return tool.confirmationRequired ? "Write · 需确认" : "Write";
}

function connectorAuthLabel(connector: ConnectorInfo): string {
  if (connector.definition.id === "github") {
    return "GitHub 登录 / GitHub CLI / 高级凭证";
  }
  if (connector.definition.id === "notion") {
    return "Notion 浏览器登录 / 高级凭证";
  }
  if (connector.definition.id === "slack") {
    return "Slack 浏览器登录 / 高级凭证";
  }
  if (connector.definition.id === "gmail") {
    return "Gmail 邮箱账号 / 应用专用密码";
  }
  if (connector.definition.id === "qq_mail") {
    return "QQ 邮箱账号 / 授权码";
  }
  if (connector.definition.id === "netease_mail") {
    return "网易邮箱账号 / 授权码";
  }
  switch (connector.definition.authType) {
    case "none":
      return "无需认证";
    case "oauth":
      return connector.definition.envVars.length > 0
        ? "浏览器 OAuth / 高级凭证"
        : "浏览器 OAuth";
    case "apiKey":
      return "官方授权 / 高级凭证";
    case "externalMcp":
      return "外部 MCP / 插件";
    default:
      return connector.definition.envVars.length > 0
        ? "官方授权 / 高级凭证"
        : "外部授权";
  }
}

function authSourceLabel(source?: string | null): string | null {
  switch (source) {
    case "environment":
      return "环境变量";
    case "mail_credentials":
      return "邮箱授权码";
    case "oauth_device":
      return "Omiga OAuth";
    case "oauth_browser":
      return "浏览器 OAuth";
    case "github_cli":
      return "GitHub CLI";
    case "codex_apps":
    case "mcp_app":
      return "Codex/OpenAI Apps";
    case "manual":
      return "旧版本地状态";
    default:
      return source ? source.replace(/[_-]+/g, " ") : null;
  }
}

function connectorStatusText(connector: ConnectorInfo): string {
  if (connector.accessible) return "已连接";
  if (!connector.enabled) return "未添加";
  if (connector.status === "metadata_only") return "仅元数据";
  return "需要认证";
}

function connectorSupportsLogin(connector: ConnectorInfo): boolean {
  return (
    connector.definition.id === "github" ||
    connector.definition.id === "notion" ||
    connector.definition.id === "slack"
  );
}

function connectorHasMissingProductOAuthConfig(
  connector: ConnectorInfo,
  errorMessage: string,
): boolean {
  const normalized = errorMessage.toLowerCase();
  if (connector.definition.id === "slack") {
    return (
      normalized.includes("omiga_slack_oauth") ||
      normalized.includes("slack oauth requires") ||
      normalized.includes("slack browser login requires")
    );
  }
  if (connector.definition.id === "notion") {
    return (
      normalized.includes("omiga_notion_oauth") ||
      normalized.includes("notion browser login requires")
    );
  }
  if (connector.definition.id === "github") {
    return (
      normalized.includes("omiga_github_oauth") ||
      normalized.includes("github oauth login requires")
    );
  }
  if (connector.definition.id === "gmail") {
    return (
      normalized.includes("omiga_gmail_oauth") ||
      normalized.includes("omiga_google_oauth") ||
      normalized.includes("gmail browser login requires")
    );
  }
  return false;
}

export function connectorLoginFailureMessage(
  connector: ConnectorInfo,
  errorMessage: string,
  openedHostedPage = false,
): string {
  const action = openedHostedPage ? "已尝试启动 Omiga 授权；" : "";
  if (connectorHasMissingProductOAuthConfig(connector, errorMessage)) {
    return `${action}${connector.definition.name} 的 Omiga 登录服务尚未在当前构建中启用。请更新到包含该连接器登录服务的 Omiga 版本，或由应用提供方接入 Omiga 自有 OAuth 服务；不会跳转到 OpenAI/Codex 托管授权页。`;
  }
  return errorMessage || `${connector.definition.name} 登录启动失败。`;
}

function connectorConnectionCtaLabel(connector: ConnectorInfo): string {
  if (!connectorIsProductIntegrated(connector)) return "暂未接入";
  if (connector.accessible) return "已连接";
  if (connectorSupportsLogin(connector)) return `连接 ${connector.definition.name}`;
  if (connectorSupportsCredentialValidation(connector)) return `连接 ${connector.definition.name}`;
  return "暂未接入";
}

function connectorAuthFlowButtonLabel(flow: ConnectorAuthFlow): string {
  if (flow.status === "opening") return "打开中…";
  if (flow.status === "checking") return "检测中…";
  if (flow.status === "setup_required") return "继续连接";
  if (flow.status === "error") return "重新连接";
  return "等待授权";
}

function connectorAuthFlowIsBusy(flow?: ConnectorAuthFlow): boolean {
  return (
    flow?.status === "opening" ||
    flow?.status === "waiting" ||
    flow?.status === "checking"
  );
}

function connectorAuthFlowNeedsAttention(flow?: ConnectorAuthFlow): boolean {
  return flow?.status === "setup_required" || flow?.status === "error";
}

function connectorAuthFlowTitle(
  connector: ConnectorInfo,
  flow?: ConnectorAuthFlow,
): string {
  if (flow?.status === "opening") return "正在打开授权页";
  if (flow?.status === "checking") return "正在检测授权";
  if (flow?.status === "waiting") return "等待授权完成";
  if (flow?.status === "setup_required") {
    return connectorSupportsCredentialValidation(connector)
      ? "需要邮箱账号和授权码"
      : "登录服务未就绪";
  }
  if (flow?.status === "error") return "授权未完成";
  if (connector.accessible) return "连接已可用";
  if (connectorSupportsLogin(connector)) return "需要授权";
  if (connectorSupportsCredentialValidation(connector)) return "需要连接";
  return "等待真实接入";
}

function connectorAuthFlowChipColor(
  flow: ConnectorAuthFlow,
): "info" | "warning" | "error" {
  if (flow.status === "setup_required") return "warning";
  if (flow.status === "error") return "error";
  return "info";
}

function connectorSetupHints(connector: ConnectorInfo): string[] {
  switch (connector.definition.id) {
    case "gmail":
      return ["在上方输入 Gmail 地址和 Google 应用专用密码。"];
    case "notion":
      return [
        "当前版本未内置 Notion 的 Omiga 登录服务。",
        "请更新应用或安装提供 Notion OAuth 的 Omiga 连接器组件。",
      ];
    case "slack":
      return [
        "当前版本未内置 Slack 的 Omiga 登录服务。",
        "请更新应用或安装提供 Slack OAuth 的 Omiga 连接器组件。",
      ];
    case "github":
      return ["可以使用 GitHub CLI 登录；OAuth 服务未内置时 Omiga 会自动尝试本机 GitHub CLI。"];
    case "qq_mail":
      return ["在上方输入 QQ 邮箱地址和邮箱授权码。"];
    case "netease_mail":
      return ["在上方输入网易邮箱地址和邮箱授权码。"];
    default:
      return [];
  }
}

function connectorAuthFlowMessage(
  connector: ConnectorInfo,
  flow?: ConnectorAuthFlow,
): string {
  if (!flow) {
    if (connector.accessible) return "已连接，可以在对话中使用该连接器。";
    if (connectorSupportsLogin(connector)) return "点击连接后会在默认浏览器打开授权页。";
    if (connectorSupportsCredentialValidation(connector)) {
      return "输入邮箱账号和授权码后，Omiga 会保存到系统安全存储并校验连接状态。";
    }
    return "暂未接入真实登录或工具执行方式。";
  }
  return flow.message;
}

function connectorStartMessage(connectorName: string): string {
  return `正在打开 ${connectorName} 授权页…`;
}

function connectorWaitingMessage(
  connector: ConnectorInfo,
  result: ConnectorLoginStartResult,
): string {
  if (result.provider === "github_cli") {
    return "已启动 GitHub CLI 登录。完成后 Omiga 会自动检测。";
  }
  if (result.provider === "github" && result.userCode) {
    return `已打开 GitHub 授权页，输入代码 ${result.userCode} 后会自动检测。`;
  }
  return `已打开 ${connector.definition.name} 授权页，完成后会自动检测。`;
}

function connectorPollWaitingMessage(
  connector: ConnectorInfo,
  result: ConnectorLoginPollResult,
): string {
  if (result.status === "slow_down") return "服务要求放慢检测频率，稍后继续自动检测。";
  return `等待 ${connector.definition.name} 浏览器授权完成…`;
}

function OmigaIconTile({ size = 64 }: { size?: number }) {
  return (
    <Box
      aria-hidden="true"
      sx={{
        width: size,
        height: size,
        borderRadius: 2.5,
        display: "grid",
        placeItems: "center",
        bgcolor: "common.black",
        color: "common.white",
        boxShadow: "0 16px 32px rgba(0,0,0,0.18)",
      }}
    >
      <Typography
        component="span"
        sx={{
          fontSize: Math.round(size * 0.43),
          fontWeight: 950,
          letterSpacing: -1.2,
          lineHeight: 1,
        }}
      >
        O
      </Typography>
    </Box>
  );
}

function connectorConnectRows(connector: ConnectorInfo): Array<{
  title: string;
  body: string;
}> {
  if (connectorSupportsCredentialValidation(connector)) {
    return [
      {
        title: "输入邮箱账号和授权码",
        body: "这是面向普通用户的连接流程，不需要手动配置环境变量。",
      },
      {
        title: "Omiga 保存到系统安全存储",
        body: "授权码不会写入 connectors/config.json，也不会显示在连接器列表中。",
      },
      {
        title: "自动校验连接状态",
        body: "保存后会立即检测默认 IMAP 端点，确认该邮箱连接可以使用。",
      },
    ];
  }
  if (connectorSupportsLogin(connector)) {
    return [
      {
        title: "此页面将打开默认浏览器",
        body: `你将登录并在 ${connector.definition.name} 的页面确认权限。`,
      },
      {
        title: "使用 Omiga 自有授权逻辑",
        body: "不会跳转到 OpenAI/Codex 托管授权页；授权完成后 Omiga 会自动检测状态。",
      },
      {
        title: "一切由你掌控",
        body: "Connector 只会在已声明的权限范围内读取或执行操作，你可以随时断开连接。",
      },
    ];
  }
  return [
    {
      title: "暂未接入真实连接方式",
      body: "当前只展示连接器元数据。接入 OAuth、本机软件登录或原生工具后才能连接。",
    },
  ];
}

function ConnectorConnectDialog({
  connector,
  open,
  busy,
  testing,
  testResult,
  authFlow,
  onClose,
  onConnect,
}: {
  connector: ConnectorInfo | null;
  open: boolean;
  busy: boolean;
  testing: boolean;
  testResult?: ConnectorConnectionTestResult;
  authFlow?: ConnectorAuthFlow;
  onClose: () => void;
  onConnect: (
    connector: ConnectorInfo,
    credentials?: MailConnectorCredentialRequest,
  ) => void;
}) {
  const theme = useTheme();
  const [mailDraft, setMailDraft] = useState<MailCredentialDraft>({
    emailAddress: "",
    authorizationCode: "",
  });
  const [mailDraftError, setMailDraftError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !connector) return;
    setMailDraft({
      emailAddress: connector.accountLabel ?? "",
      authorizationCode: "",
    });
    setMailDraftError(null);
  }, [connector?.accountLabel, connector?.definition.id, open]);

  if (!connector) return null;

  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const isMailCredentialFlow = connectorSupportsCredentialValidation(connector);
  const displayAuthFlow =
    isMailCredentialFlow && authFlow?.provider !== "credential_validation"
      ? undefined
      : authFlow;
  const mailDraftComplete =
    mailDraft.emailAddress.trim().length > 0 &&
    mailDraft.authorizationCode.trim().length > 0;
  const authFlowBusy = connectorAuthFlowIsBusy(displayAuthFlow);
  const authFlowNeedsAttention = connectorAuthFlowNeedsAttention(displayAuthFlow);
  const setupHints =
    displayAuthFlow?.status === "setup_required" ? connectorSetupHints(connector) : [];
  const isWorking = busy || testing || authFlowBusy;
  const canConnect =
    isProductIntegrated &&
    !connector.accessible &&
    connector.status !== "metadata_only" &&
    !isWorking &&
    (!isMailCredentialFlow || mailDraftComplete);
  const primaryLabel = connector.accessible
    ? `${connector.definition.name} 已连接`
    : displayAuthFlow
      ? connectorAuthFlowButtonLabel(displayAuthFlow)
      : isWorking
        ? "正在连接…"
        : connectorConnectionCtaLabel(connector);
  const tools = connector.definition.tools.slice(0, 6);
  const handleConnect = () => {
    if (isMailCredentialFlow) {
      if (!mailDraftComplete) {
        setMailDraftError("请输入邮箱地址和授权码。");
        return;
      }
      setMailDraftError(null);
      onConnect(connector, {
        connectorId: connector.definition.id,
        emailAddress: mailDraft.emailAddress.trim(),
        authorizationCode: mailDraft.authorizationCode.trim(),
      });
      return;
    }
    onConnect(connector);
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      fullWidth
      maxWidth="sm"
      aria-labelledby="connector-connect-title"
      PaperProps={{
        sx: {
          borderRadius: 4,
          overflow: "hidden",
          bgcolor: "background.paper",
          boxShadow: `0 28px 80px ${alpha(theme.palette.common.black, theme.palette.mode === "dark" ? 0.55 : 0.18)}`,
        },
      }}
    >
      <DialogContent sx={{ p: 0 }}>
        <Box sx={{ position: "relative" }}>
          <IconButton
            aria-label="关闭连接界面"
            onClick={onClose}
            sx={{
              position: "absolute",
              top: 14,
              right: 14,
              zIndex: 2,
              border: 1,
              borderColor: "divider",
              bgcolor: alpha(theme.palette.background.paper, 0.82),
              backdropFilter: "blur(10px)",
              "&:hover": { bgcolor: "background.paper" },
            }}
          >
            <CloseRounded />
          </IconButton>

          <Stack
            spacing={2.5}
            sx={{
              px: { xs: 2.5, sm: 4 },
              pt: { xs: 4, sm: 4.5 },
              pb: 2.5,
              alignItems: "center",
              textAlign: "center",
            }}
          >
            <Stack direction="row" spacing={2} alignItems="center">
              <OmigaIconTile size={64} />
              <Typography
                aria-hidden="true"
                color="text.disabled"
                sx={{ fontSize: 26, letterSpacing: 2 }}
              >
                •••
              </Typography>
              <ConnectorIcon
                connector={connector}
                size={64}
                rounded={2.5}
                disabled={!isProductIntegrated}
              />
            </Stack>
            <Box>
              <Typography
                id="connector-connect-title"
                variant="h4"
                fontWeight={950}
                letterSpacing={-0.35}
              >
                {connector.accessible
                  ? `${connector.definition.name} 已连接`
                  : `连接 ${connector.definition.name}`}
              </Typography>
              <Typography
                variant="body1"
                color="text.secondary"
                fontWeight={700}
                sx={{ mt: 0.75 }}
              >
                {connectorDeveloperLabel(connector)}
              </Typography>
            </Box>
          </Stack>

          <Box
            sx={{
              px: { xs: 2.5, sm: 4 },
              pb: 2,
              maxHeight: { xs: "58vh", sm: "60vh" },
              overflowY: "auto",
            }}
          >
            <Paper
              variant="outlined"
              sx={{
                borderRadius: 3,
                overflow: "hidden",
                bgcolor:
                  theme.palette.mode === "dark"
                    ? alpha(theme.palette.background.default, 0.4)
                    : "background.paper",
              }}
            >
              <Stack
                direction="row"
                spacing={1.5}
                alignItems="center"
                sx={{ px: 2, py: 1.7 }}
              >
                <ConnectorIcon
                  connector={connector}
                  size={42}
                  rounded={1.5}
                  disabled={!isProductIntegrated}
                />
                <Box sx={{ minWidth: 0, flex: 1, textAlign: "left" }}>
                  <Stack
                    direction="row"
                    spacing={1}
                    alignItems="center"
                    flexWrap="wrap"
                    useFlexGap
                  >
                    <Typography variant="subtitle1" fontWeight={900}>
                      {connector.definition.name}
                    </Typography>
                    {connector.source !== "built_in" && (
                      <Chip
                        size="small"
                        variant="outlined"
                        label={sourceLabel(connector)}
                      />
                    )}
                  </Stack>
                  <Typography variant="body2" color="text.secondary">
                    {connector.definition.description}
                  </Typography>
                </Box>
              </Stack>

              <Stack sx={{ borderTop: 1, borderColor: "divider" }}>
                {connectorConnectRows(connector).map((row) => (
                  <Box
                    key={row.title}
                    sx={{ px: 2, py: 1.45, borderBottom: 1, borderColor: "divider" }}
                  >
                    <Typography
                      variant="body2"
                      fontWeight={900}
                      sx={{ textAlign: "left" }}
                    >
                      {row.title}
                    </Typography>
                    <Typography
                      variant="body2"
                      color="text.secondary"
                      sx={{ mt: 0.35, textAlign: "left", lineHeight: 1.55 }}
                    >
                      {row.body}
                    </Typography>
                  </Box>
                ))}
              </Stack>

              {isMailCredentialFlow && !connector.accessible && (
                <Box sx={{ px: 2, py: 1.6, borderTop: 1, borderColor: "divider" }}>
                  <Stack spacing={1.25}>
                    <Typography
                      variant="body2"
                      fontWeight={900}
                      sx={{ textAlign: "left" }}
                    >
                      邮箱账号
                    </Typography>
                    <TextField
                      size="small"
                      label="邮箱地址"
                      value={mailDraft.emailAddress}
                      onChange={(event) => {
                        setMailDraft((prev) => ({
                          ...prev,
                          emailAddress: event.target.value,
                        }));
                        setMailDraftError(null);
                      }}
                      placeholder={
                        connector.definition.id === "gmail"
                          ? "name@gmail.com"
                          : connector.definition.id === "qq_mail"
                            ? "name@qq.com"
                            : "name@163.com"
                      }
                      autoComplete="email"
                      fullWidth
                    />
                    <TextField
                      size="small"
                      label="邮箱授权码 / 应用专用密码"
                      type="password"
                      value={mailDraft.authorizationCode}
                      onChange={(event) => {
                        setMailDraft((prev) => ({
                          ...prev,
                          authorizationCode: event.target.value,
                        }));
                        setMailDraftError(null);
                      }}
                      autoComplete="current-password"
                      fullWidth
                    />
                    <Typography
                      variant="caption"
                      color="text.secondary"
                      sx={{ textAlign: "left", lineHeight: 1.5 }}
                    >
                      {connector.definition.id === "gmail"
                        ? "应用专用密码由 Google 账号生成；Omiga 会写入系统安全存储，并使用 Gmail 默认 IMAP 服务自动检测。"
                        : "授权码由邮箱服务商生成；Omiga 会写入系统安全存储，并使用默认 IMAP 服务自动检测。"}
                    </Typography>
                    {mailDraftError && (
                      <Alert severity="warning" variant="outlined" sx={{ borderRadius: 2 }}>
                        {mailDraftError}
                      </Alert>
                    )}
                  </Stack>
                </Box>
              )}

              {(displayAuthFlow || testResult || connector.accessible) && (
                <Box sx={{ px: 2, py: 1.6, borderTop: 1, borderColor: "divider" }}>
                  <Stack spacing={1} alignItems="stretch">
                    <Stack
                      direction="row"
                      spacing={1}
                      alignItems="center"
                      flexWrap="wrap"
                      useFlexGap
                    >
                      <Typography
                        variant="body2"
                        fontWeight={900}
                        sx={{ textAlign: "left", flex: 1 }}
                      >
                        {connectorAuthFlowTitle(connector, displayAuthFlow)}
                      </Typography>
                      {displayAuthFlow && (
                        <Chip
                          size="small"
                          color={connectorAuthFlowChipColor(displayAuthFlow)}
                          variant="outlined"
                          label={connectorAuthFlowButtonLabel(displayAuthFlow)}
                        />
                      )}
                      {connector.accessible && (
                        <Chip size="small" color="success" label="已连接" />
                      )}
                    </Stack>
                    <Typography
                      variant="body2"
                      color="text.secondary"
                      sx={{ textAlign: "left", lineHeight: 1.55 }}
                    >
                      {displayAuthFlow
                        ? connectorAuthFlowMessage(connector, displayAuthFlow)
                        : testResult?.message ||
                          connectorAuthFlowMessage(connector, displayAuthFlow)}
                    </Typography>
                    {authFlowNeedsAttention && (
                      <Alert
                        severity={
                          displayAuthFlow?.status === "setup_required"
                            ? "warning"
                            : "error"
                        }
                        variant="outlined"
                        sx={{ borderRadius: 2, textAlign: "left" }}
                      >
                        {displayAuthFlow?.message}
                      </Alert>
                    )}
                    {setupHints.length > 0 && (
                      <Stack
                        direction="row"
                        spacing={0.75}
                        flexWrap="wrap"
                        useFlexGap
                      >
                        {setupHints.map((hint) => (
                          <Chip
                            key={hint}
                            size="small"
                            variant="outlined"
                            label={hint}
                            sx={{
                              fontFamily:
                                hint.includes("OMIGA_") ||
                                hint.startsWith("Redirect:")
                                  ? "monospace"
                                  : undefined,
                            }}
                          />
                        ))}
                      </Stack>
                    )}
                  </Stack>
                </Box>
              )}

              <Box sx={{ px: 2, py: 1.6, borderTop: 1, borderColor: "divider" }}>
                <Typography
                  variant="body2"
                  fontWeight={900}
                  sx={{ mb: 1, textAlign: "left" }}
                >
                  包含内容
                </Typography>
                <Stack
                  direction="row"
                  spacing={0.75}
                  flexWrap="wrap"
                  useFlexGap
                >
                  <Chip
                    size="small"
                    variant="outlined"
                    label={`${connector.definition.name} 应用`}
                  />
                  {tools.map((tool) => (
                    <Chip
                      key={tool.name}
                      size="small"
                      variant="outlined"
                      color={tool.readOnly ? "default" : "warning"}
                      label={`${tool.name} · ${tool.readOnly ? "Read" : "Write"}`}
                    />
                  ))}
                </Stack>
              </Box>
            </Paper>
          </Box>

          <Box
            sx={{
              px: { xs: 2.5, sm: 4 },
              pt: 1,
              pb: { xs: 2.5, sm: 3 },
            }}
          >
            <Button
              fullWidth
              size="large"
              variant="contained"
              disabled={!canConnect}
              onClick={handleConnect}
              startIcon={
                isWorking ? <CircularProgress size={18} color="inherit" /> : null
              }
              sx={{
                minHeight: 52,
                borderRadius: 999,
                bgcolor: "text.primary",
                color: "background.paper",
                fontWeight: 900,
                fontSize: 16,
                boxShadow: "none",
                "&:hover": {
                  bgcolor: "text.secondary",
                  boxShadow: `0 12px 30px ${alpha(theme.palette.common.black, 0.18)}`,
                },
              }}
            >
              {primaryLabel}
            </Button>
          </Box>
        </Box>
      </DialogContent>
    </Dialog>
  );
}

function ConnectorDetailsDialog({
  connector,
  open,
  busy,
  testing,
  testResult,
  auditEvents,
  authFlow,
  onClose,
  onDisconnect,
  onTest,
  onOpenConnect,
}: {
  connector: ConnectorInfo | null;
  open: boolean;
  busy: boolean;
  testing: boolean;
  testResult?: ConnectorConnectionTestResult;
  auditEvents: ConnectorAuditEvent[];
  authFlow?: ConnectorAuthFlow;
  onClose: () => void;
  onDisconnect: (connector: ConnectorInfo) => void;
  onTest: (connector: ConnectorInfo) => void;
  onOpenConnect: (connector: ConnectorInfo) => void;
}) {
  const theme = useTheme();

  if (!connector) return null;

  const metadataOnly = connector.status === "metadata_only";
  const isProductIntegrated = connectorIsProductIntegrated(connector);
  const toolCount = connector.definition.tools.length;
  const connectionTestHistory = connector.connectionTestHistory ?? [];
  const previousTestResults = connectionTestHistory
    .filter(
      (item) =>
        !testResult ||
        item.connectorId !== testResult.connectorId ||
        item.checkedAt !== testResult.checkedAt,
    )
    .slice(0, 3);
  const healthDetail = connectionHealthDetail(connector.connectionHealth);
  const infoRows = [
    ["类别", `${sourceLabel(connector)}, ${categoryLabel(connector.definition.category)}`],
    ["功能", connectorCapabilitiesLabel(connector)],
    ["执行", connectorRuntimeLabel(connector)],
    ["开发者", connectorDeveloperLabel(connector).replace(/^由\s*/, "")],
    ["认证", connectorAuthLabel(connector)],
    ["状态", connectorStatusText(connector)],
    ["存储", "用户级，密钥不写入配置文件"],
  ];
  const authFlowBusy = connectorAuthFlowIsBusy(authFlow);
  const authFlowNeedsAttention = connectorAuthFlowNeedsAttention(authFlow);
  const setupHints =
    authFlow?.status === "setup_required" ? connectorSetupHints(connector) : [];
  const primaryActionLabel = connectorConnectionCtaLabel(connector);
  const canStartLogin = connectorSupportsLogin(connector);
  const canValidateCredentials = connectorSupportsCredentialValidation(connector);
  const primaryActionDisabled =
    !isProductIntegrated ||
    connector.accessible ||
    metadataOnly ||
    (!canStartLogin && !canValidateCredentials);
  const handlePrimaryAction = () => {
    onOpenConnect(connector);
  };

  return (
      <Dialog
        open={open}
        onClose={onClose}
        fullWidth
        maxWidth="lg"
        aria-labelledby="connector-details-title"
        PaperProps={{
          sx: {
            borderRadius: 3,
            overflow: "hidden",
            maxHeight: "92vh",
          },
        }}
      >
        <DialogContent
          sx={{
            p: 0,
            bgcolor:
              theme.palette.mode === "dark"
                ? alpha(theme.palette.common.black, 0.22)
                : "background.default",
          }}
        >
          <Box
            sx={{
              maxWidth: 920,
              mx: "auto",
              px: { xs: 2.5, md: 4 },
              py: { xs: 3, md: 4 },
            }}
          >
            <Stack spacing={{ xs: 3, md: 3.5 }}>
              <Stack
                direction="row"
                spacing={1.25}
                alignItems="center"
                sx={{ color: "text.secondary" }}
              >
                <Typography variant="body2" fontWeight={700}>
                  连接器
                </Typography>
                <Typography aria-hidden="true">›</Typography>
                <Typography
                  id="connector-details-title"
                  variant="body2"
                  color="text.primary"
                  fontWeight={900}
                  sx={{ flex: 1 }}
                >
                  {connector.definition.name}
                </Typography>
                <Tooltip title="复制连接器 ID">
                  <IconButton
                    size="small"
                    aria-label="复制连接器 ID"
                    onClick={() =>
                      void navigator.clipboard.writeText(connector.definition.id)
                    }
                  >
                    <LinkRounded fontSize="small" />
                  </IconButton>
                </Tooltip>
                <Button
                  variant="contained"
                  disabled={primaryActionDisabled}
                  onClick={handlePrimaryAction}
                  startIcon={
                    busy || testing || authFlowBusy ? (
                      <CircularProgress size={16} color="inherit" />
                    ) : undefined
                  }
                  sx={{
                    borderRadius: 999,
                    bgcolor: isProductIntegrated
                      ? "text.primary"
                      : "action.disabledBackground",
                    color: isProductIntegrated
                      ? "background.paper"
                      : "text.disabled",
                    fontWeight: 900,
                    px: 2.25,
                    "&:hover": {
                      bgcolor: isProductIntegrated
                        ? "text.secondary"
                        : "action.disabledBackground",
                    },
                  }}
                >
                  {authFlow
                    ? connectorAuthFlowButtonLabel(authFlow)
                    : busy || testing
                      ? "正在连接"
                      : primaryActionLabel}
                </Button>
                <IconButton aria-label="关闭连接器详情" onClick={onClose}>
                  <CloseRounded />
                </IconButton>
              </Stack>

              <Stack spacing={2.5}>
                <ConnectorIcon
                  connector={connector}
                  size={64}
                  rounded={2.5}
                  disabled={!isProductIntegrated}
                />
                <Box>
                  <Typography variant="h4" fontWeight={950} letterSpacing={-0.4}>
                    {connector.definition.name}
                  </Typography>
                  <Typography variant="h6" color="text.secondary" fontWeight={500} sx={{ mt: 0.75 }}>
                    {connector.definition.description}
                  </Typography>
                </Box>
              </Stack>

              {!isProductIntegrated && (
                <Alert severity="info" sx={{ borderRadius: 2 }}>
                  {connector.definition.name} 暂未接入真实软件登录和可执行工具，
                  当前仅展示灰色元数据，不能启用或连接。后续接入 OAuth / 本地软件登录 /
                  native 工具后会自动变为可用。
                </Alert>
              )}

              <Box
                sx={{
                  borderRadius: 4,
                  overflow: "hidden",
                  minHeight: { xs: 180, md: 210 },
                  display: "grid",
                  placeItems: "center",
                  px: 3,
                  background:
                    !isProductIntegrated
                      ? `linear-gradient(135deg, ${alpha(theme.palette.text.disabled, 0.18)}, ${alpha(theme.palette.text.disabled, 0.08)})`
                      : theme.palette.mode === "dark"
                      ? `linear-gradient(135deg, ${alpha(theme.palette.success.dark, 0.18)}, ${alpha(theme.palette.warning.dark, 0.12)}), radial-gradient(circle at 20% 20%, ${alpha(theme.palette.common.white, 0.12)}, transparent 28%)`
                      : `linear-gradient(135deg, ${alpha(theme.palette.primary.light, 0.38)}, ${alpha(theme.palette.secondary.light, 0.28)}), radial-gradient(circle at 18% 22%, ${alpha(theme.palette.common.white, 0.8)}, transparent 30%)`,
                }}
              >
                <Paper
                  variant="outlined"
                  sx={{
                    maxWidth: 620,
                    px: 2,
                    py: 1.5,
                    borderRadius: 3,
                    bgcolor: alpha(theme.palette.background.paper, theme.palette.mode === "dark" ? 0.76 : 0.72),
                    backdropFilter: "blur(18px)",
                  }}
                >
                  <Stack direction="row" spacing={1.25} alignItems="center">
                    <ConnectorIcon
                      connector={connector}
                      size={28}
                      rounded={1}
                      disabled={!isProductIntegrated}
                    />
                    <Typography variant="body1" fontWeight={800}>
                      {connector.definition.name}
                    </Typography>
                    <Typography variant="body1" color="text.secondary">
                      {connector.definition.description}
                    </Typography>
                  </Stack>
                </Paper>
              </Box>

              <Typography variant="body1" sx={{ lineHeight: 1.65 }}>
                使用 {connector.definition.name} 访问外部服务、读取必要上下文并执行已声明的工具能力。
                连接器优先走用户级登录/授权，密钥由系统安全存储或外部工具管理。
              </Typography>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  包含内容
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  <Stack
                    direction="row"
                    spacing={2}
                    alignItems="center"
                    sx={{ px: 2, py: 1.7 }}
                  >
                    <ConnectorIcon
                      connector={connector}
                      size={44}
                      rounded="50%"
                      disabled={!isProductIntegrated}
                    />
                    <Box sx={{ minWidth: 0, flex: 1 }}>
                      <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                        <Typography variant="subtitle1" fontWeight={900}>
                          {connector.definition.name}
                        </Typography>
                        <Typography variant="body2" color="text.secondary">
                          应用
                        </Typography>
                      </Stack>
                      <Typography variant="body2" color="text.secondary" noWrap>
                        {authHint(connector)}
                      </Typography>
                    </Box>
                    {connector.enabled && <CheckCircleRounded color="success" fontSize="small" />}
                  </Stack>
                  {connector.definition.tools.map((tool) => (
                    <Stack
                      key={tool.name}
                      direction="row"
                      spacing={2}
                      alignItems="center"
                      sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}
                    >
                      <Box
                        sx={{
                          width: 44,
                          height: 44,
                          borderRadius: "50%",
                          display: "grid",
                          placeItems: "center",
                          border: 1,
                          borderColor: "divider",
                          color: "text.secondary",
                          flexShrink: 0,
                        }}
                      >
                        <TroubleshootRounded fontSize="small" />
                      </Box>
                      <Box sx={{ minWidth: 0, flex: 1 }}>
                        <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap">
                          <Typography variant="subtitle1" fontWeight={900}>
                            {tool.name}
                          </Typography>
                          <Chip
                            size="small"
                            color={toolExecutionColor(tool.execution)}
                            variant="outlined"
                            label={toolExecutionLabel(tool.execution)}
                          />
                          <Chip
                            size="small"
                            variant="outlined"
                            color={tool.readOnly ? "default" : "warning"}
                            label={toolAccessLabel(tool)}
                          />
                        </Stack>
                        <Typography variant="body2" color="text.secondary" noWrap>
                          {tool.description}
                        </Typography>
                        {tool.requiredScopes.length > 0 && (
                          <Stack
                            direction="row"
                            spacing={0.75}
                            flexWrap="wrap"
                            useFlexGap
                            sx={{ mt: 0.75 }}
                          >
                            <Typography
                              variant="caption"
                              color="text.secondary"
                              sx={{ alignSelf: "center", fontWeight: 700 }}
                            >
                              权限
                            </Typography>
                            {tool.requiredScopes.map((scope) => (
                              <Chip
                                key={`${tool.name}-${scope}`}
                                size="small"
                                variant="outlined"
                                label={scope}
                                sx={{ height: 22 }}
                              />
                            ))}
                          </Stack>
                        )}
                      </Box>
                    </Stack>
                  ))}
                  {toolCount === 0 && (
                    <Box sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}>
                      <Typography variant="body2" color="text.secondary">
                        暂未声明原生工具。接入原生执行器或 MCP 后会显示能力。
                      </Typography>
                    </Box>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  信息
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  {infoRows.map(([label, value], index) => (
                    <Stack
                      key={label}
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      sx={{
                        px: 2,
                        py: 1.7,
                        borderTop: index === 0 ? 0 : 1,
                        borderColor: "divider",
                      }}
                    >
                      <Typography variant="body2" color="text.secondary" sx={{ width: 220, flexShrink: 0 }}>
                        {label}
                      </Typography>
                      <Typography variant="body2" fontWeight={700} sx={{ minWidth: 0, wordBreak: "break-word" }}>
                        {value}
                      </Typography>
                    </Stack>
                  ))}
                  {(connector.definition.installUrl || connector.definition.docsUrl) && (
                    <Stack
                      direction={{ xs: "column", sm: "row" }}
                      spacing={1}
                      sx={{ px: 2, py: 1.7, borderTop: 1, borderColor: "divider" }}
                    >
                      <Typography variant="body2" color="text.secondary" sx={{ width: 220, flexShrink: 0 }}>
                        链接
                      </Typography>
                      <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                        {connector.definition.installUrl && (
                          <Button
                            size="small"
                            variant="text"
                            endIcon={<OpenInNewRounded />}
                            onClick={() => void openUrl(connector.definition.installUrl!)}
                          >
                            连接文档
                          </Button>
                        )}
                        {connector.definition.docsUrl && (
                          <Button
                            size="small"
                            variant="text"
                            endIcon={<OpenInNewRounded />}
                            onClick={() => void openUrl(connector.definition.docsUrl!)}
                          >
                            API 文档
                          </Button>
                        )}
                      </Stack>
                    </Stack>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Typography variant="h6" fontWeight={900}>
                  最近操作
                </Typography>
                <Paper variant="outlined" sx={{ borderRadius: 3, overflow: "hidden", bgcolor: "background.paper" }}>
                  {auditEvents.length > 0 ? (
                    auditEvents.slice(0, 5).map((event, index) => (
                      <Stack
                        key={event.id}
                        direction={{ xs: "column", md: "row" }}
                        spacing={1.5}
                        alignItems={{ xs: "flex-start", md: "center" }}
                        sx={{
                          px: 2,
                          py: 1.6,
                          borderTop: index === 0 ? 0 : 1,
                          borderColor: "divider",
                        }}
                      >
                        <Box sx={{ minWidth: 0, flex: 1 }}>
                          <Stack direction="row" spacing={1} alignItems="center" flexWrap="wrap" useFlexGap>
                            <Typography variant="subtitle2" fontWeight={900}>
                              {event.operation}
                            </Typography>
                            <Chip
                              size="small"
                              variant="outlined"
                              color={event.access === "write" ? "warning" : "default"}
                              label={event.access === "write" ? "Write" : "Read"}
                            />
                            <Chip
                              size="small"
                              variant="outlined"
                              color={auditOutcomeColor(event.outcome)}
                              label={auditOutcomeLabel(event.outcome)}
                            />
                            {event.confirmationRequired && (
                              <Chip
                                size="small"
                                variant="outlined"
                                label={event.confirmed ? "已确认" : "未确认"}
                              />
                            )}
                          </Stack>
                          <Typography variant="body2" color="text.secondary" sx={{ mt: 0.4 }}>
                            {event.target ? `目标：${event.target}` : "未记录目标"} ·{" "}
                            {formatCheckedAt(event.createdAt)}
                          </Typography>
                          {event.message && (
                            <Typography
                              variant="caption"
                              color="text.secondary"
                              sx={{ mt: 0.35, display: "block" }}
                            >
                              {event.message}
                            </Typography>
                          )}
                        </Box>
                        {event.sessionId && (
                          <Chip
                            size="small"
                            variant="outlined"
                            label={`session ${event.sessionId}`}
                            sx={{ maxWidth: 220 }}
                          />
                        )}
                      </Stack>
                    ))
                  ) : (
                    <Box sx={{ px: 2, py: 1.7 }}>
                      <Typography variant="body2" color="text.secondary">
                        暂无 connector 工具调用记录。读取或写入外部服务后会在这里显示审计事件。
                      </Typography>
                    </Box>
                  )}
                </Paper>
              </Stack>

              <Stack spacing={2}>
                <Stack
                  direction={{ xs: "column", sm: "row" }}
                  spacing={1}
                  alignItems={{ xs: "flex-start", sm: "center" }}
                >
                  <Typography variant="h6" fontWeight={900} sx={{ flex: 1 }}>
                    连接状态
                  </Typography>
                  <Chip
                    size="small"
                    color={statusColor(connector.status)}
                    label={connectorStatusText(connector)}
                  />
                </Stack>
                <Paper
                  variant="outlined"
                  sx={{
                    borderRadius: 3,
                    overflow: "hidden",
                    bgcolor:
                      theme.palette.mode === "dark"
                        ? alpha(theme.palette.background.paper, 0.72)
                        : "background.paper",
                    borderColor:
                      !connector.accessible && connector.enabled
                        ? alpha(theme.palette.warning.main, 0.28)
                        : "divider",
                  }}
                >
                  <Box
                    sx={{
                      px: 2,
                      py: 1.75,
                      borderBottom: 1,
                      borderColor: "divider",
                      bgcolor:
                        !connector.accessible && connector.enabled
                          ? alpha(
                              theme.palette.warning.main,
                              theme.palette.mode === "dark" ? 0.08 : 0.05,
                            )
                          : alpha(
                              theme.palette.success.main,
                              connector.accessible ? 0.06 : 0,
                            ),
                    }}
                  >
                    <Stack
                      direction={{ xs: "column", md: "row" }}
                      spacing={1.5}
                      alignItems={{ xs: "stretch", md: "center" }}
                    >
                      <Stack spacing={0.75} sx={{ minWidth: 0, flex: 1 }}>
                        <Stack
                          direction="row"
                          spacing={1}
                          alignItems="center"
                          flexWrap="wrap"
                          useFlexGap
                        >
                          <Typography variant="subtitle1" fontWeight={900}>
                            {connectorAuthFlowTitle(connector, authFlow)}
                          </Typography>
                          {authFlow && (
                            <Chip
                              size="small"
                              color={connectorAuthFlowChipColor(authFlow)}
                              variant="outlined"
                              label={connectorAuthFlowButtonLabel(authFlow)}
                            />
                          )}
                          {(connector.connectionHealth?.totalChecks ?? 0) > 0 && (
                            <Chip
                              size="small"
                              color={connectionHealthColor(
                                connector.connectionHealth,
                              )}
                              variant="outlined"
                              label={connectionHealthLabel(
                                connector.connectionHealth,
                              )}
                            />
                          )}
                          {connector.accountLabel && (
                            <Chip
                              size="small"
                              variant="outlined"
                              label={`账号：${connector.accountLabel}`}
                            />
                          )}
                          {authSourceLabel(connector.authSource) && (
                            <Chip
                              size="small"
                              variant="outlined"
                              label={`来源：${authSourceLabel(connector.authSource)}`}
                            />
                          )}
                        </Stack>
                        <Typography variant="body2" color="text.secondary">
                          {connectorAuthFlowMessage(connector, authFlow)}
                        </Typography>
                        {authFlowNeedsAttention && (
                          <Alert
                            severity={
                              authFlow?.status === "setup_required"
                                ? "warning"
                                : "error"
                            }
                            variant="outlined"
                            sx={{
                              mt: 0.5,
                              borderRadius: 2,
                              "& .MuiAlert-message": { width: "100%" },
                            }}
                          >
                            <Stack spacing={0.75}>
                              <Typography variant="body2" fontWeight={800}>
                                {authFlow?.status === "setup_required"
                                  ? connectorSupportsCredentialValidation(connector)
                                    ? "邮箱授权码未就绪，暂时无法完成连接。"
                                    : "未生成授权 URL，无法打开浏览器登录。"
                                  : "登录流程已停止，可重试连接。"}
                              </Typography>
                              {setupHints.length > 0 && (
                                <Stack
                                  direction="row"
                                  spacing={0.75}
                                  flexWrap="wrap"
                                  useFlexGap
                                >
                                  {setupHints.map((hint) => (
                                    <Chip
                                      key={hint}
                                      size="small"
                                      variant="outlined"
                                      label={hint}
                                      sx={{
                                        fontFamily:
                                          hint.includes("OMIGA_") ||
                                          hint.startsWith("Redirect:")
                                            ? "monospace"
                                            : undefined,
                                      }}
                                    />
                                  ))}
                                </Stack>
                              )}
                            </Stack>
                          </Alert>
                        )}
                        {connector.connectedAt && (
                          <Typography variant="caption" color="text.secondary">
                            连接于 {formatCheckedAt(connector.connectedAt)}
                          </Typography>
                        )}
                      </Stack>

                      <Stack
                        direction="row"
                        spacing={1}
                        flexWrap="wrap"
                        useFlexGap
                        justifyContent={{ xs: "flex-start", md: "flex-end" }}
                      >
                        {!connector.accessible && connectorSupportsLogin(connector) && (
                          <Button
                            size="small"
                            variant="contained"
                            startIcon={
                              authFlowBusy ? (
                                <CircularProgress size={14} color="inherit" />
                              ) : (
                                <LinkRounded />
                              )
                            }
                            disabled={metadataOnly}
                            onClick={() => onOpenConnect(connector)}
                            sx={{ borderRadius: 999, fontWeight: 800 }}
                          >
                            {authFlow
                              ? connectorAuthFlowButtonLabel(authFlow)
                              : `连接 ${connector.definition.name}`}
                          </Button>
                        )}
                        {!connector.accessible &&
                          !connectorSupportsLogin(connector) &&
                          connectorSupportsCredentialValidation(connector) && (
                            <Button
                              size="small"
                              variant="contained"
                              startIcon={
                                testing ? (
                                  <CircularProgress size={14} color="inherit" />
                                ) : (
                                  <TroubleshootRounded />
                                )
                              }
                              disabled={metadataOnly}
                              onClick={() => onOpenConnect(connector)}
                              sx={{ borderRadius: 999, fontWeight: 800 }}
                            >
                              连接 {connector.definition.name}
                            </Button>
                          )}
                        <Button
                          size="small"
                          variant="outlined"
                          startIcon={
                            testing ? (
                              <CircularProgress size={14} />
                            ) : (
                              <TroubleshootRounded />
                            )
                          }
                          disabled={busy || testing}
                          onClick={() => onTest(connector)}
                          sx={{ borderRadius: 999 }}
                        >
                          检测连接
                        </Button>
                      </Stack>
                    </Stack>
                  </Box>

                  <Stack spacing={1.25} sx={{ p: 2 }}>
                    {testResult ? (
                      <Paper
                        variant="outlined"
                        sx={{
                          borderRadius: 2.5,
                          p: 1.5,
                          bgcolor: alpha(
                            testResult.ok
                              ? theme.palette.success.main
                              : theme.palette.warning.main,
                            theme.palette.mode === "dark" ? 0.08 : 0.045,
                          ),
                          borderColor: alpha(
                            testResult.ok
                              ? theme.palette.success.main
                              : theme.palette.warning.main,
                            0.32,
                          ),
                        }}
                      >
                        <Stack direction="row" spacing={1.25} alignItems="flex-start">
                          <Box
                            sx={{
                              width: 32,
                              height: 32,
                              borderRadius: "50%",
                              display: "grid",
                              placeItems: "center",
                              color: testResult.ok
                                ? "success.main"
                                : "warning.main",
                              flexShrink: 0,
                            }}
                          >
                            {testResult.ok ? (
                              <CheckCircleRounded fontSize="small" />
                            ) : (
                              <TroubleshootRounded fontSize="small" />
                            )}
                          </Box>
                          <Box sx={{ minWidth: 0, flex: 1 }}>
                            <Typography variant="body2" fontWeight={900}>
                              {testKindLabel(testResult)} ·{" "}
                              {testResult.ok ? "连接正常" : "需要处理"}
                            </Typography>
                            <Typography variant="body2" color="text.secondary">
                              {testResult.message}
                            </Typography>
                            <Stack
                              direction="row"
                              spacing={0.75}
                              flexWrap="wrap"
                              useFlexGap
                              sx={{ mt: 0.75 }}
                            >
                              <Chip
                                size="small"
                                variant="outlined"
                                label={`检测于 ${formatCheckedAt(testResult.checkedAt)}`}
                              />
                              {testDetailChips(testResult).map((chip) => (
                                <Chip
                                  key={chip.key}
                                  size="small"
                                  color={chip.color ?? "default"}
                                  variant="outlined"
                                  label={chip.label}
                                />
                              ))}
                            </Stack>
                            {testResult.details && (
                              <Typography
                                variant="caption"
                                color="text.secondary"
                                sx={{ mt: 0.75, display: "block" }}
                              >
                                {testResult.details}
                              </Typography>
                            )}
                          </Box>
                        </Stack>
                      </Paper>
                    ) : (
                      <Typography variant="body2" color="text.secondary">
                        还没有连接测试结果。完成官方授权或本机软件登录后，可以运行一次检测确认可用性。
                      </Typography>
                    )}

                    {healthDetail && (
                      <Typography variant="body2" color="text.secondary">
                        Recent health: {healthDetail}
                      </Typography>
                    )}
                    {previousTestResults.length > 0 && (
                      <Stack direction="row" spacing={0.75} flexWrap="wrap" useFlexGap>
                        {previousTestResults.map((item) => (
                          <Chip
                            key={`${item.connectorId}-${item.checkedAt}`}
                            size="small"
                            color={item.ok ? "success" : "warning"}
                            variant="outlined"
                            label={`${item.ok ? "OK" : (item.errorCode ?? "Failed")} · ${formatCheckedAt(item.checkedAt)}`}
                          />
                        ))}
                      </Stack>
                    )}

                    <Stack direction="row" spacing={1} flexWrap="wrap" useFlexGap>
                      {connector.accessible ? (
                        <Button
                          size="small"
                          variant="outlined"
                          color="inherit"
                          startIcon={<CloudOffRounded />}
                          disabled={busy || metadataOnly}
                          onClick={() => onDisconnect(connector)}
                        >
                          断开连接
                        </Button>
                      ) : null}
                    </Stack>
                  </Stack>
                </Paper>
              </Stack>
            </Stack>
          </Box>
        </DialogContent>
      </Dialog>
  );
}

export function ConnectorsPanel({
  projectPath,
}: {
  projectPath: string;
}) {
  const {
    catalog,
    auditEvents,
    isLoading,
    isMutating,
    testingConnectorIds,
    testResults,
    error,
    loadConnectors,
    loadConnectorAuditEvents,
    setConnectorEnabled,
    disconnectConnector,
    saveMailConnectorCredentials,
    testConnectorConnection,
    startConnectorLogin,
    pollConnectorLogin,
  } = useConnectorStore();
  const [notice, setNotice] = useState<ConnectorNotice | null>(null);
  const [connectorAuthFlows, setConnectorAuthFlows] = useState<
    Record<string, ConnectorAuthFlow>
  >({});
  const [searchQuery, setSearchQuery] = useState("");
  const deferredSearchQuery = useDeferredValue(searchQuery);
  const [statusFilter, setStatusFilter] =
    useState<ConnectorStatusFilter>("all");
  const [sourceFilter, setSourceFilter] =
    useState<ConnectorSourceFilter>("all");
  const [categoryFilter, setCategoryFilter] = useState("all");
  const [detailConnectorId, setDetailConnectorId] = useState<string | null>(
    null,
  );
  const [connectConnectorId, setConnectConnectorId] = useState<string | null>(
    null,
  );

  const hasCatalog = Boolean(catalog);

  useEffect(() => {
    void loadConnectors({ background: hasCatalog });
  }, [hasCatalog, loadConnectors]);

  useEffect(() => {
    if (!detailConnectorId) return;
    void loadConnectorAuditEvents(detailConnectorId);
  }, [detailConnectorId, loadConnectorAuditEvents]);

  const categories = useMemo(() => {
    return Array.from(
      new Set(
        (catalog?.connectors ?? []).map(
          (connector) => connector.definition.category || "other",
        ),
      ),
    ).sort((left, right) => left.localeCompare(right));
  }, [catalog]);

  const connectorsById = useMemo(() => {
    const byId = new Map<string, ConnectorInfo>();
    for (const connector of catalog?.connectors ?? []) {
      byId.set(connector.definition.id, connector);
    }
    return byId;
  }, [catalog]);

  const detailConnector = detailConnectorId
    ? (connectorsById.get(detailConnectorId) ?? null)
    : null;
  const connectConnector = connectConnectorId
    ? (connectorsById.get(connectConnectorId) ?? null)
    : null;
  const detailAuditEvents = useMemo(
    () =>
      detailConnectorId
        ? auditEvents.filter((event) => event.connectorId === detailConnectorId)
        : [],
    [auditEvents, detailConnectorId],
  );

  const connectorTestResult = useCallback((
    connector: ConnectorInfo,
  ): ConnectorConnectionTestResult | undefined =>
    testResults[connector.definition.id] ??
    connector.lastConnectionTest ??
    undefined,
  [testResults]);

  const filteredConnectors = useMemo(() => {
    return (catalog?.connectors ?? []).filter((connector) => {
      if (!connectorMatchesSearch(connector, deferredSearchQuery)) return false;
      if (statusFilter !== "all" && connector.status !== statusFilter)
        return false;
      if (sourceFilter !== "all" && connector.source !== sourceFilter)
        return false;
      if (
        categoryFilter !== "all" &&
        connector.definition.category !== categoryFilter
      ) {
        return false;
      }
      return true;
    });
  }, [
    catalog,
    categoryFilter,
    deferredSearchQuery,
    sourceFilter,
    statusFilter,
  ]);

  const grouped = useMemo(() => {
    const groups = new Map<string, ConnectorInfo[]>();
    for (const connector of filteredConnectors) {
      const category = connector.definition.category || "other";
      const categoryConnectors = groups.get(category);
      if (categoryConnectors) {
        categoryConnectors.push(connector);
      } else {
        groups.set(category, [connector]);
      }
    }
    return Array.from(groups.entries()).sort(([left], [right]) =>
      left.localeCompare(right),
    );
  }, [filteredConnectors]);

  const startConnectorAuth = useCallback(
    async (connector: ConnectorInfo) => {
      const connectorId = connector.definition.id;
      setNotice(null);
      setDetailConnectorId(null);
      setConnectConnectorId(connectorId);
      setConnectorAuthFlows((prev) => ({
        ...prev,
        [connectorId]: {
          loginSessionId: "",
          provider: "",
          status: "opening",
          intervalSecs: 2,
          message: connectorStartMessage(connector.definition.name),
        },
      }));

      try {
        if (!connector.enabled) {
          await setConnectorEnabled(connectorId, true);
        }
        const result = await startConnectorLogin(connectorId);
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            loginSessionId: result.loginSessionId,
            provider: result.provider,
            status: "waiting",
            intervalSecs: Math.max(1, result.intervalSecs || 2),
            message: connectorWaitingMessage(connector, result),
          },
        }));

        const authUrl = result.verificationUriComplete ?? result.verificationUri;
        if (result.provider !== "github_cli" && authUrl) {
          await openUrl(authUrl);
        }
      } catch (error) {
        const errorMessage = extractErrorMessage(error);
        const message = connectorLoginFailureMessage(connector, errorMessage, false);
        const status: ConnectorAuthFlowStatus =
          connectorHasMissingProductOAuthConfig(connector, errorMessage)
            ? "setup_required"
            : "error";
        setConnectConnectorId(connectorId);
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            loginSessionId: "",
            provider: connector.definition.id,
            status,
            intervalSecs: 0,
            message,
          },
        }));
      }
    },
    [setConnectorEnabled, startConnectorLogin],
  );

  const pollConnectorAuthLogin = useCallback(
    async (connectorId: string, flow: ConnectorAuthFlow) => {
      if (!flow.loginSessionId) return;
      const connector = connectorsById.get(connectorId);
      if (!connector) return;
      setConnectorAuthFlows((prev) => {
        const current = prev[connectorId];
        if (!current || current.loginSessionId !== flow.loginSessionId) return prev;
        return {
          ...prev,
          [connectorId]: {
            ...current,
            status: "checking",
            message: "正在检测授权状态…",
          },
        };
      });

      try {
        const result = await pollConnectorLogin(flow.loginSessionId);
        if (result.status === "pending" || result.status === "slow_down") {
          setConnectorAuthFlows((prev) => {
            const current = prev[connectorId];
            if (!current || current.loginSessionId !== flow.loginSessionId) {
              return prev;
            }
            return {
              ...prev,
              [connectorId]: {
                ...current,
                status: "waiting",
                intervalSecs: Math.max(1, result.intervalSecs || current.intervalSecs),
                message: connectorPollWaitingMessage(connector, result),
              },
            };
          });
          return;
        }

        if (result.status === "complete") {
          setConnectorAuthFlows((prev) => {
            const next = { ...prev };
            delete next[connectorId];
            return next;
          });
          return;
        }
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            ...flow,
            status: "error",
            message: result.message || `${connector.definition.name} 授权失败。`,
          },
        }));
      } catch (error) {
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            ...flow,
            status: "error",
            message: `检测 ${connector.definition.name} 授权状态失败：${extractErrorMessage(error)}`,
          },
        }));
      }
    },
    [connectorsById, pollConnectorLogin],
  );

  useEffect(() => {
    const timers = Object.entries(connectorAuthFlows)
      .filter(([, flow]) => flow.status === "waiting")
      .map(([connectorId, flow]) =>
        window.setTimeout(
          () => void pollConnectorAuthLogin(connectorId, flow),
          Math.max(1, flow.intervalSecs || 2) * 1000,
        ),
      );
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [connectorAuthFlows, pollConnectorAuthLogin]);

  useEffect(() => {
    if (!catalog) return;
    setConnectorAuthFlows((prev) => {
      let changed = false;
      const next = { ...prev };
      for (const connector of catalog.connectors) {
        if (connector.accessible && next[connector.definition.id]) {
          delete next[connector.definition.id];
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [catalog]);

  const testConnectorAndUpdateFlow = useCallback(
    (connector: ConnectorInfo) => {
      const connectorId = connector.definition.id;
      setNotice(null);
      setDetailConnectorId(null);
      setConnectConnectorId(connectorId);
      setConnectorAuthFlows((prev) => ({
        ...prev,
        [connectorId]: {
          loginSessionId: "",
          provider: "credential_validation",
          status: "checking",
          intervalSecs: 0,
          message: `正在连接 ${connector.definition.name}…`,
        },
      }));

      void (async () => {
        if (!connector.enabled) {
          await setConnectorEnabled(connectorId, true);
        }
        const result = await testConnectorConnection(connectorId, projectPath);
        if (result.ok) {
          setConnectorAuthFlows((prev) => {
            const next = { ...prev };
            delete next[connectorId];
            return next;
          });
          return;
        }
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            loginSessionId: "",
            provider: "credential_validation",
            status: "setup_required",
            intervalSecs: 0,
            message:
              result.message ||
              `${connector.definition.name} 需要邮箱账号和授权码才能连接。`,
          },
        }));
      })().catch((error) => {
        const message = extractErrorMessage(error);
        setConnectorAuthFlows((prev) => ({
          ...prev,
          [connectorId]: {
            loginSessionId: "",
            provider: "credential_validation",
            status: "error",
            intervalSecs: 0,
            message: `连接 ${connector.definition.name} 失败：${message}`,
          },
        }));
      });
    },
    [projectPath, setConnectorEnabled, testConnectorConnection],
  );

  const openConnectorConnect = useCallback(
    (connector: ConnectorInfo) => {
      const connectorId = connector.definition.id;
      if (connectorSupportsCredentialValidation(connector)) {
        setConnectorAuthFlows((prev) => {
          const flow = prev[connectorId];
          if (!flow || flow.provider === "credential_validation") return prev;
          const next = { ...prev };
          delete next[connectorId];
          return next;
        });
      }
      setDetailConnectorId(null);
      setConnectConnectorId(connectorId);
    },
    [],
  );

  const openConnectorDetails = useCallback((connector: ConnectorInfo) => {
    setDetailConnectorId(connector.definition.id);
  }, []);

  const runConnectorConnection = useCallback(
    (
      connector: ConnectorInfo,
      credentials?: MailConnectorCredentialRequest,
    ) => {
      if (connectorSupportsCredentialValidation(connector)) {
        if (credentials) {
          const connectorId = connector.definition.id;
          setNotice(null);
          setDetailConnectorId(null);
          setConnectConnectorId(connectorId);
          setConnectorAuthFlows((prev) => ({
            ...prev,
            [connectorId]: {
              loginSessionId: "",
              provider: "credential_validation",
              status: "checking",
              intervalSecs: 0,
              message: `正在保存 ${connector.definition.name} 授权信息…`,
            },
          }));
          void (async () => {
            if (!connector.enabled) {
              await setConnectorEnabled(connectorId, true);
            }
            const savedConnector = await saveMailConnectorCredentials(credentials);
            testConnectorAndUpdateFlow(savedConnector);
          })().catch((error) => {
            setConnectorAuthFlows((prev) => ({
              ...prev,
              [connectorId]: {
                loginSessionId: "",
                provider: "credential_validation",
                status: "error",
                intervalSecs: 0,
                message: `保存 ${connector.definition.name} 授权信息失败：${extractErrorMessage(error)}`,
              },
            }));
          });
          return;
        }
        testConnectorAndUpdateFlow(connector);
        return;
      }
      if (connectorSupportsLogin(connector)) {
        void startConnectorAuth(connector);
        return;
      }
      setConnectConnectorId(connector.definition.id);
    },
    [
      saveMailConnectorCredentials,
      setConnectorEnabled,
      startConnectorAuth,
      testConnectorAndUpdateFlow,
    ],
  );

  return (
    <Box sx={{ mt: 2 }}>
      <Stack spacing={2}>
        <Stack direction="row" spacing={1} alignItems="center">
          <Typography variant="h6" fontWeight={700} sx={{ flex: 1 }}>
            Connectors
          </Typography>
          <Chip
            size="small"
            variant="outlined"
            label={`Scope: ${catalog?.scope ?? "user"}`}
          />
          {catalog?.configPath && (
            <Tooltip title={catalog.configPath}>
              <IconButton
                size="small"
                onClick={() =>
                  void navigator.clipboard.writeText(catalog.configPath)
                }
              >
                <ContentCopyRounded fontSize="small" />
              </IconButton>
            </Tooltip>
          )}
        </Stack>

        <Card variant="outlined" sx={{ borderRadius: 2 }}>
          <CardContent>
            <Stack spacing={1.5}>
              <Stack direction={{ xs: "column", md: "row" }} spacing={1.5}>
                <TextField
                  size="small"
                  label="Search connectors"
                  value={searchQuery}
                  onChange={(event) => setSearchQuery(event.target.value)}
                  fullWidth
                />
                <TextField
                  size="small"
                  label="Category"
                  value={categoryFilter}
                  onChange={(event) => setCategoryFilter(event.target.value)}
                  select
                  sx={{ minWidth: 180 }}
                >
                  <MenuItem value="all">All</MenuItem>
                  {categories.map((category) => (
                    <MenuItem key={category} value={category}>
                      {categoryLabel(category)}
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  size="small"
                  label="Status"
                  value={statusFilter}
                  onChange={(event) =>
                    setStatusFilter(event.target.value as ConnectorStatusFilter)
                  }
                  select
                  sx={{ minWidth: 170 }}
                >
                  <MenuItem value="all">All</MenuItem>
                  {(
                    [
                      "connected",
                      "needs_auth",
                      "disabled",
                      "metadata_only",
                    ] as const
                  ).map((status) => (
                    <MenuItem key={status} value={status}>
                      {statusLabel(status)}
                    </MenuItem>
                  ))}
                </TextField>
                <TextField
                  size="small"
                  label="Source"
                  value={sourceFilter}
                  onChange={(event) =>
                    setSourceFilter(event.target.value as ConnectorSourceFilter)
                  }
                  select
                  sx={{ minWidth: 150 }}
                >
                  <MenuItem value="all">All</MenuItem>
                  {(["built_in", "plugin"] as const).map((source) => (
                    <MenuItem key={source} value={source}>
                      {sourceValueLabel(source)}
                    </MenuItem>
                  ))}
                </TextField>
              </Stack>
            </Stack>
          </CardContent>
        </Card>

        {notice && (
          <Alert severity={notice.severity} onClose={() => setNotice(null)}>
            {notice.message}
          </Alert>
        )}

        {error && <Alert severity="error">{error}</Alert>}

        {isLoading && !catalog ? (
          <Box sx={{ py: 4, textAlign: "center" }}>
            <CircularProgress size={24} />
          </Box>
        ) : grouped.length === 0 ? (
          <Alert severity="warning">
            {catalog?.connectors.length
              ? "No connectors match the current filters."
              : "No connector definitions found."}
          </Alert>
        ) : (
          grouped.map(([category, connectors]) => (
            <Box key={category}>
              <Typography
                variant="subtitle2"
                color="text.secondary"
                sx={{ mb: 1 }}
              >
                {categoryLabel(category)}
              </Typography>
              <Box sx={connectorCardGridSx}>
                {connectors.map((connector) => (
                  <ConnectorCard
                    key={connector.definition.id}
                    connector={connector}
                    busy={
                      isMutating ||
                      Boolean(testingConnectorIds[connector.definition.id]) ||
                      connectorAuthFlowIsBusy(
                        connectorAuthFlows[connector.definition.id],
                      )
                    }
                    testResult={connectorTestResult(connector)}
                    onQuickAdd={openConnectorConnect}
                    onOpenDetails={openConnectorDetails}
                  />
                ))}
              </Box>
            </Box>
          ))
        )}
      </Stack>
      <ConnectorDetailsDialog
        connector={detailConnector}
        open={Boolean(detailConnector)}
        busy={isMutating}
        testing={Boolean(
          detailConnector
            ? testingConnectorIds[detailConnector.definition.id]
            : false,
        )}
        testResult={
          detailConnector ? connectorTestResult(detailConnector) : undefined
        }
        auditEvents={detailAuditEvents}
        authFlow={
          detailConnector
            ? connectorAuthFlows[detailConnector.definition.id]
            : undefined
        }
        onClose={() => setDetailConnectorId(null)}
        onDisconnect={(item) => void disconnectConnector(item.definition.id)}
        onTest={testConnectorAndUpdateFlow}
        onOpenConnect={openConnectorConnect}
      />
      <ConnectorConnectDialog
        connector={connectConnector}
        open={Boolean(connectConnector)}
        busy={isMutating}
        testing={Boolean(
          connectConnector
            ? testingConnectorIds[connectConnector.definition.id]
            : false,
        )}
        testResult={
          connectConnector ? connectorTestResult(connectConnector) : undefined
        }
        authFlow={
          connectConnector
            ? connectorAuthFlows[connectConnector.definition.id]
            : undefined
        }
        onClose={() => setConnectConnectorId(null)}
        onConnect={runConnectorConnection}
      />
    </Box>
  );
}
