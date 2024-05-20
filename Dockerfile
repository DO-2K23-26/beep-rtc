# Use rust image as base
FROM rust:latest

# Set working directory inside the container
WORKDIR /usr/src/app

# Copy the Rust project files into the container
COPY . .

# Build the Rust project
RUN cargo build --release --package beep-sfu

# Expose the TCP port the signal server will listen on
EXPOSE 8080
# Expose the UDP ports the media server will listen on
EXPOSE 3478-3495/udp

# Command to run the server
CMD ./target/release/beep-sfu -d --level info -e dev
