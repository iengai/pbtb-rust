
pub mod client;
pub mod botrepository;
pub mod configtemplaterepository;
pub mod botconfigrepository;
pub mod apikeyrepository;

pub use botrepository::DynamoBotRepository;
pub use configtemplaterepository::S3TemplateRepository;
pub use botconfigrepository::S3BotConfigRepository;
pub use apikeyrepository::S3ApiKeyRepository;