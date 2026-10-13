/**
 * `@spacekit/sdk/plugins`: describe, check, install and load SpaceKit plugins.
 * See `sdks/spacekit-apps/SPACEKIT-PLUGIN-SPEC.md`.
 */

export {
  BRAIN_FILE_ROLES,
  ENGINE_WEIGHTS_ROLE,
  weightsRole,
  BUILDING_BLOCKS,
  PLUGIN_DESCRIPTOR_FILE,
  PLUGIN_DESCRIPTOR_VERSION,
  PLUGIN_KINDS,
  PluginDescriptorError,
  checkConfigValue,
  parsePluginDescriptor,
  validatePluginDescriptor,
  versionSatisfies,
  type BaseModel,
  type BrainAbility,
  type BrainSpec,
  type BuildingBlock,
  type ConfigField,
  type ConfigFieldType,
  type ContractSpec,
  type PluginDependency,
  type PluginDescriptor,
  type PluginFile,
  type PluginKind,
  type PluginPricing,
  type PluginValidation,
  type PricingModel,
  type ServiceMethod,
  type ServiceSpec,
  type UiSpec,
} from "./descriptor.js";

export {
  PluginConfigError,
  checkPluginPermissions,
  describePlugin,
  installScopeLabel,
  pluginCapabilityPolicy,
  pluginInstallScope,
  resolvePluginConfig,
  type PermissionCheck,
  type PluginInstallScope,
} from "./install.js";

export {
  BrainVerificationError,
  fetchVerifiedBrainFiles,
  growformerEngine,
  growformerInstanceEngine,
  httpBrainFileFetcher,
  loadBrainPlugin,
  type BrainEngine,
  type BrainFileFetcher,
  type BrainLoadProgress,
  type GrowformerInstanceLike,
  type GrowformerRuntimeExports,
  type LoadBrainPluginOptions,
  type LoadedBrain,
} from "./brain.js";

export {
  PLUGIN_SIGNATURE_DOMAIN,
  pluginSigningMessage,
  verifyPluginDescriptorSignature,
  type PluginSignatureResult,
} from "./signature.js";

export {
  PLUGIN_REQUEST_TOPIC,
  PluginCallError,
  PluginRouter,
  checkServiceParams,
  type InstalledPlugin,
} from "./router.js";

export {
  PluginInstallError,
  createPluginInstallClient,
  requirePluginLicense,
  type PluginApiOptions,
  type PluginInstallClient,
  type PluginInstallRecord,
} from "./license.js";
