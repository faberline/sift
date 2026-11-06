//! OpenAPI belongs to the assembly boundary. These descriptions are never used
//! to copy or encode journal records. Golden tests pin the whole document.

macro_rules! schema_adapter {
    ($model:ty, $description:ty) => {
        impl<'s> utoipa::ToSchema<'s> for $model {
            fn schema() -> (
                &'s str,
                utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
            ) {
                <$description as utoipa::ToSchema<'s>>::schema()
            }

            fn aliases() -> Vec<(&'s str, utoipa::openapi::schema::Schema)> {
                <$description as utoipa::ToSchema<'s>>::aliases()
            }
        }
    };
}

mod append_result;
mod event;
mod log;
mod metric;
mod stored_event;
mod trace;
