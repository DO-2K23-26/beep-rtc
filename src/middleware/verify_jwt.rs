use actix_web::dev::ServiceRequest;

pub fn verify_token(req: ServiceRequest) -> bool {
    // Add your JWT verification logic here
    // For example, extract JWT from headers and make a request to external API for verification
    // return true if JWT is valid, else false
    true
}