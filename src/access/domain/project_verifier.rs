//! Check credentials for one project and one pure route role.

use crate::access::domain::route_role::RouteRole;
use crate::shared_kernel::port_future::PortFuture;

pub(crate) trait ProjectVerifier: Send + Sync {
    type Credentials: ?Sized;
    type Principal;
    type Error;
    fn is_kubernetes(&self) -> bool;
    fn authenticate_project<'a>(
        &'a self,
        credentials: &'a Self::Credentials,
        project: &'a str,
        role: RouteRole,
    ) -> PortFuture<'a, Self::Principal, Self::Error>;
}
