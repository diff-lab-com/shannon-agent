// Go fixture: small server module, used by the multi-language walk test.

package server

type Config struct {
	Host string
	Port int
}

type Server struct {
	cfg Config
}

func New(cfg Config) *Server {
	return &Server{cfg: cfg}
}

func (s *Server) Start() error {
	return nil
}

func (s *Server) Stop() error {
	return nil
}

type Handler interface {
	Handle(req string) (string, error)
}

const DefaultPort = 8080
